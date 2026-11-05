// JNI contains only the Android platform boundary. Streaming orchestration and
// bounded wire framing are shared Rust; Camera2 and TLS use Android's APIs.
use camerabridge_core::{choose_fps, write_packet};
use jni::{
    objects::{JByteArray, JClass, JIntArray, JObject, JString, JValue},
    sys::jint,
    JNIEnv,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Condvar, Mutex, OnceLock,
    },
    time::Duration,
};

#[derive(Default)]
struct EncoderQueue {
    packets: VecDeque<(Vec<u8>, i32)>,
    config: Vec<u8>,
    ready: bool,
    encoded: i32,
}
impl EncoderQueue {
    fn push(&mut self, mut data: Vec<u8>, flags: i32, rotation: i32) -> bool {
        if flags & 2 != 0 {
            self.config = data;
            return false;
        }
        self.encoded += 1;
        let key = flags & 1 != 0;
        if !self.ready && !key {
            return false;
        }
        if self.packets.len() >= 4 {
            self.packets.clear();
            self.ready = false;
            return true;
        }
        if key {
            let mut complete = self.config.clone();
            complete.append(&mut data);
            data = complete;
            self.ready = true;
        }
        if data.len() > camerabridge_core::MAX_PACKET {
            self.packets.clear();
            self.ready = false;
            return true;
        }
        self.packets.push_back((data, rotation));
        false
    }
}
type Queue = Arc<(Mutex<EncoderQueue>, Condvar)>;
static QUEUES: OnceLock<Mutex<HashMap<i64, Queue>>> = OnceLock::new();
static NEXT: AtomicI64 = AtomicI64::new(1);
fn queues() -> &'static Mutex<HashMap<i64, Queue>> {
    QUEUES.get_or_init(Default::default)
}
fn queue(id: i64) -> Option<Queue> {
    queues().lock().unwrap().get(&id).cloned()
}
#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_queueCreate(_: JNIEnv, _: JClass) -> i64 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    queues().lock().unwrap().insert(
        id,
        Arc::new((Mutex::new(EncoderQueue::default()), Condvar::new())),
    );
    id
}
#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_queueReset(
    _: JNIEnv,
    _: JClass,
    id: i64,
) {
    if let Some(q) = queue(id) {
        *q.0.lock().unwrap() = EncoderQueue::default();
        q.1.notify_all();
    }
}
#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_queueDestroy(
    _: JNIEnv,
    _: JClass,
    id: i64,
) {
    queues().lock().unwrap().remove(&id);
}
#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_queueConfig(
    mut env: JNIEnv,
    _: JClass,
    id: i64,
    bytes: JByteArray,
) {
    match env.convert_byte_array(bytes) {
        Ok(b) => {
            if let Some(q) = queue(id) {
                q.0.lock().unwrap().config = b;
            }
        }
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_queuePush(
    mut env: JNIEnv,
    _: JClass,
    id: i64,
    bytes: JByteArray,
    flags: jint,
    sensor: jint,
    display: jint,
    front: u8,
) -> u8 {
    match env.convert_byte_array(bytes) {
        Ok(data) => {
            if let Some(q) = queue(id) {
                let rotation = if front != 0 {
                    (sensor + display) % 360
                } else {
                    (sensor - display + 360) % 360
                };
                let sync = q.0.lock().unwrap().push(data, flags, rotation);
                q.1.notify_one();
                sync as u8
            } else {
                0
            }
        }
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
            0
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_queuePoll(
    mut env: JNIEnv,
    _: JClass,
    id: i64,
    timeout: i64,
) -> jni::sys::jobject {
    let result = (|| -> jni::errors::Result<jni::sys::jobject> {
        let Some(q) = queue(id) else {
            return Ok(std::ptr::null_mut());
        };
        let guard = q.0.lock().unwrap();
        let (mut guard, _) =
            q.1.wait_timeout_while(
                guard,
                Duration::from_millis(timeout.clamp(0, 1000) as u64),
                |q| q.packets.is_empty(),
            )
            .unwrap();
        let Some((data, rotation)) = guard.packets.pop_front() else {
            return Ok(std::ptr::null_mut());
        };
        drop(guard);
        let bytes = env.byte_array_from_slice(&data)?;
        Ok(env
            .new_object(
                "app/camerabridge/CameraEngine$Packet",
                "([BI)V",
                &[JValue::Object(&bytes), JValue::Int(rotation)],
            )?
            .into_raw())
    })();
    match result {
        Ok(v) => v,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
            std::ptr::null_mut()
        }
    }
}
#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_queueEncoded(
    _: JNIEnv,
    _: JClass,
    id: i64,
) -> jint {
    queue(id)
        .map(|q| {
            let mut q = q.0.lock().unwrap();
            std::mem::take(&mut q.encoded)
        })
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    #[test]
    fn congestion_requires_new_keyframe() {
        let mut q = EncoderQueue::default();
        q.push(vec![9], 2, 0);
        q.push(vec![1], 0, 0);
        assert!(q.packets.is_empty());
        q.push(vec![2], 1, 90);
        assert_eq!(q.packets[0], (vec![9, 2], 90));
        for _ in 0..3 {
            assert!(!q.push(vec![3], 0, 0));
        }
        assert!(q.push(vec![4], 0, 0));
        assert!(q.packets.is_empty());
        q.push(vec![5], 0, 0);
        assert!(q.packets.is_empty());
        q.push(vec![6], 1, 0);
        assert_eq!(q.packets[0].0, vec![9, 6]);
    }
}

#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_chooseFps(
    mut env: JNIEnv,
    _: JClass,
    ranges: JIntArray,
    target: jint,
    steady: jint,
) -> jint {
    let result = (|| -> jni::errors::Result<i32> {
        let mut flat = vec![0; env.get_array_length(&ranges)? as usize];
        env.get_int_array_region(&ranges, 0, &mut flat)?;
        let pairs: Vec<_> = flat
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| (v[0], v[1]))
            .collect();
        Ok(choose_fps(&pairs, target, steady != 0))
    })();
    match result {
        Ok(v) => v,
        Err(e) => {
            let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
            -1
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_app_camerabridge_NativeBridge_run(
    mut env: JNIEnv,
    _: JClass,
    service: JObject,
) {
    if let Err(e) = network(&mut env, &service) {
        if !env.exception_check().unwrap_or(true) {
            let _ = env.throw_new("java/lang/IllegalStateException", e.to_string());
        }
    }
}
fn call_bool(env: &mut JNIEnv, obj: &JObject, name: &str) -> jni::errors::Result<bool> {
    env.call_method(obj, name, "()Z", &[])?.z()
}
fn network(env: &mut JNIEnv, service: &JObject) -> jni::errors::Result<()> {
    let mut retry = 400;
    while call_bool(env, service, "nativeArmed")? {
        // Per-connection local frame prevents JNI references accumulating over days.
        let result = env.with_local_frame(32, |env| -> jni::errors::Result<()> {
            env.call_method(service, "nativeConnect", "()V", &[])?;
            retry = 400;
            while call_bool(env, service, "nativeArmed")? {
                env.with_local_frame(16, |env| -> jni::errors::Result<()> {
                    let status = env.call_method(service, "nativeStatus", "()[B", &[])?.l()?;
                    if !status.is_null() {
                        let bytes = env.convert_byte_array(JByteArray::from(status))?;
                        send(env, service, &bytes, u32::MAX)?;
                    }
                    let packet = env
                        .call_method(
                            service,
                            "nativePoll",
                            "()Lapp/camerabridge/CameraEngine$Packet;",
                            &[],
                        )?
                        .l()?;
                    if !packet.is_null() {
                        let rotation = env.get_field(&packet, "rotation", "I")?.i()? as u32;
                        let data = env.get_field(&packet, "data", "[B")?.l()?;
                        let bytes = env.convert_byte_array(JByteArray::from(data))?;
                        send(env, service, &bytes, rotation)?;
                    }
                    env.call_method(service, "nativeTick", "()V", &[])?;
                    Ok(())
                })?;
            }
            Ok(())
        });
        if result.is_err() {
            let exception = env.exception_occurred()?;
            env.exception_clear()?;
            let text = if exception.is_null() {
                "Connection interrupted".into()
            } else {
                let s = env
                    .call_method(exception, "toString", "()Ljava/lang/String;", &[])?
                    .l()?;
                let string = JString::from(s);
                let value: String = env.get_string(&string)?.into();
                value
            };
            let msg = env.new_string(text)?;
            env.call_method(
                service,
                "nativeFailure",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&msg)],
            )?;
        }
        env.call_method(service, "nativeDisconnect", "()V", &[])?;
        if call_bool(env, service, "nativeArmed")? {
            std::thread::sleep(std::time::Duration::from_millis(retry));
            retry = (retry * 2).min(5000);
        }
    }
    Ok(())
}
fn send(
    env: &mut JNIEnv,
    service: &JObject,
    data: &[u8],
    rotation: u32,
) -> jni::errors::Result<()> {
    let mut packet = Vec::with_capacity(data.len() + 8);
    if let Err(e) = write_packet(&mut packet, data, rotation) {
        env.throw_new("java/io/IOException", e.to_string())?;
        return Err(jni::errors::Error::JavaException);
    }
    let bytes = env.byte_array_from_slice(&packet)?;
    env.call_method(service, "nativeWrite", "([B)V", &[JValue::Object(&bytes)])?;
    Ok(())
}
