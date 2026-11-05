use crate::camera::Camera;
use anyhow::{bail, Result};
use camerabridge_core::{normalize, read_json, read_packet, write_json, Settings, STATUS};
use openh264::{decoder::Decoder, formats::YUVSource};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::BufReader,
    net::{Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

pub struct State {
    pub settings: Settings,
    pub streaming: bool,
    pub revision: u32,
    pub status: String,
    pub frame: Vec<u8>,
    pub size: (usize, usize),
    pub sequence: u64,
    pub client: Option<TcpStream>,
    pub stop: bool,
}
pub struct Receiver {
    pub state: Arc<Mutex<State>>,
    pub pairing: String,
    pub directory: PathBuf,
    worker: Option<thread::JoinHandle<()>>,
}
pub fn directory() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("CameraBridge")
}
impl Receiver {
    pub fn start() -> Result<Self> {
        Self::start_at(directory(), "0.0.0.0:47950", String::new())
    }
    fn start_at(directory: PathBuf, bind: &str, suffix: String) -> Result<Self> {
        let identity = directory.join("identity");
        fs::create_dir_all(&identity)?;
        let cert = identity.join("cert.pem");
        let key = identity.join("key.pem");
        if !cert.exists() && !key.exists() {
            let pair = rcgen::generate_simple_self_signed(vec!["CameraBridge".into()])?;
            fs::write(&key, pair.key_pair.serialize_pem())?;
            fs::write(&cert, pair.cert.pem())?;
        }
        let certs = rustls_pemfile::certs(&mut BufReader::new(fs::File::open(cert)?))
            .collect::<std::io::Result<Vec<_>>>()?;
        let fingerprint = hex(&Sha256::digest(
            certs
                .first()
                .ok_or_else(|| anyhow::anyhow!("Missing certificate"))?
                .as_ref(),
        ));
        let private = rustls_pemfile::private_key(&mut BufReader::new(fs::File::open(key)?))?
            .ok_or_else(|| anyhow::anyhow!("Missing private key"))?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = Arc::new(
            rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(certs, private)?,
        );
        let token_path = identity.join("pairing-token.txt");
        let token = if token_path.exists() {
            fs::read_to_string(token_path)?.trim().to_owned()
        } else {
            let mut b = [0; 32];
            getrandom::getrandom(&mut b).map_err(|e| anyhow::anyhow!("Random: {e}"))?;
            let t = hex(&b);
            fs::write(token_path, &t)?;
            t
        };
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("Invalid stored pairing token");
        }
        let mut settings: Settings = fs::read(directory.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        if settings.resolution == "1920x1080" {
            settings.resolution = "1280x720".into();
        }
        if !settings.validate() {
            settings = Settings::default();
        }
        let state = Arc::new(Mutex::new(State {
            size: settings.size(),
            settings,
            streaming: true,
            revision: 0,
            status: "Waiting for phone".into(),
            frame: vec![],
            sequence: 0,
            client: None,
            stop: false,
        }));
        let listener = TcpListener::bind(bind)?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let host = local_ip();
        let pairing=serde_json::json!({"v":1,"host":host,"port":port,"token":token,"fingerprint":fingerprint}).to_string();
        let shared = state.clone();
        let worker = thread::Builder::new()
            .name("CameraBridge receiver".into())
            .spawn(move || {
                let mut camera = match Camera::new(&suffix) {
                    Ok(c) => c,
                    Err(e) => {
                        shared.lock().unwrap().status = format!("Webcam mapping: {e:#}");
                        return;
                    }
                };
                let blank = vec![0; 1280 * 720 * 4];
                let _ = camera.send(&blank, 1280, 720);
                loop {
                    if shared.lock().unwrap().stop {
                        break;
                    }
                    match listener.accept() {
                        Ok((raw, _)) => {
                            let result = serve(raw, &config, &token, &shared, &mut camera);
                            if let Err(error) = &result {
                                eprintln!("Receiver: {error:#}");
                            }
                            let mut s = shared.lock().unwrap();
                            s.client = None;
                            s.frame.clear();
                            s.sequence += 1;
                            s.status = match result {
                                Ok(()) => "Waiting for phone".into(),
                                Err(e) => format!("Waiting for phone · {e}"),
                            };
                            drop(s);
                            let _ = camera.send(&blank, 1280, 720);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            let _ = camera.heartbeat();
                            thread::sleep(Duration::from_millis(100));
                        }
                        Err(e) => {
                            shared.lock().unwrap().status = e.to_string();
                            break;
                        }
                    }
                }
                let _ = camera.send(&blank, 1280, 720);
            })?;
        Ok(Self {
            state,
            pairing,
            directory,
            worker: Some(worker),
        })
    }
    pub fn apply(&self, settings: Settings, streaming: bool) -> Result<()> {
        if !settings.validate() {
            bail!("Invalid settings");
        }
        // Save before changing the live configuration; preserve the old settings on failure.
        let path = self.directory.join("settings.json");
        let temp = path.with_extension("tmp");
        fs::write(&temp, serde_json::to_vec_pretty(&settings)?)?;
        fs::rename(temp, path)?;
        let mut s = self.state.lock().unwrap();
        let reconnect = s.settings.wire(s.streaming, 0) != settings.wire(streaming, 0);
        s.settings = settings;
        s.streaming = streaming;
        if reconnect {
            s.revision += 1;
            if let Some(c) = &s.client {
                let _ = c.shutdown(Shutdown::Both);
            }
        }
        Ok(())
    }
}
impl Drop for Receiver {
    fn drop(&mut self) {
        {
            let mut s = self.state.lock().unwrap();
            s.stop = true;
            if let Some(c) = &s.client {
                let _ = c.shutdown(Shutdown::Both);
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn serve(
    raw: TcpStream,
    config: &Arc<rustls::ServerConfig>,
    token: &str,
    shared: &Arc<Mutex<State>>,
    camera: &mut Camera,
) -> Result<()> {
    raw.set_nonblocking(false)?; // Windows inherits the listener's nonblocking mode.
    raw.set_nodelay(true)?;
    raw.set_read_timeout(Some(Duration::from_secs(5)))?;
    raw.set_write_timeout(Some(Duration::from_secs(5)))?;
    shared.lock().unwrap().client = Some(raw.try_clone()?);
    let mut tls = rustls::StreamOwned::new(rustls::ServerConnection::new(config.clone())?, raw);
    let hello = read_json(&mut tls)?;
    let supplied = hello["token"].as_str().unwrap_or("");
    if ![1, 2].contains(&hello["v"].as_u64().unwrap_or(0))
        || hello["codec"] != "h264"
        || !bool::from(supplied.as_bytes().ct_eq(token.as_bytes()))
    {
        bail!("Pairing rejected");
    }
    let (settings, streaming, revision) = {
        let s = shared.lock().unwrap();
        (s.settings.clone(), s.streaming, s.revision)
    };
    write_json(&mut tls, &settings.wire(streaming, revision))?;
    shared.lock().unwrap().status = "Phone connected".into();
    let mut decoder = Decoder::new()?;
    let mut packet = Vec::new();
    let mut rgb = Vec::new();
    let mut rgba = Vec::new();
    let mut count = 0;
    let mut start = Instant::now();
    loop {
        {
            let s = shared.lock().unwrap();
            if s.stop || s.revision != revision {
                return Ok(());
            }
        }
        let rotation = read_packet(&mut tls, &mut packet)?;
        if packet.is_empty() {
            camera.heartbeat()?;
            continue;
        }
        if rotation == STATUS {
            let state: serde_json::Value = serde_json::from_slice(&packet)?;
            let mut s = shared.lock().unwrap();
            if state["revision"].as_u64() == Some(revision.into()) {
                if let Some(p) = state["phone_preview"].as_bool() {
                    s.settings.phone_preview = p;
                }
                if let Some(l) = state["lens"].as_str() {
                    if ["front", "rear"].contains(&l) {
                        s.settings.lens = l.into();
                    }
                }
                if let Some(on) = state["streaming"].as_bool() {
                    s.streaming = on;
                }
                if state["active"] == false {
                    s.frame.clear();
                    s.sequence += 1;
                    let blank = vec![0; 1280 * 720 * 4];
                    camera.send(&blank, 1280, 720)?;
                }
                s.status = state["message"].as_str().unwrap_or("Phone ready").into();
            }
            continue;
        }
        if let Some(yuv) = decoder.decode(&packet)? {
            let (w, h) = yuv.dimensions();
            if w > 1280 || h > 720 {
                bail!("Video exceeds 720p capture limit");
            }
            rgb.resize(w * h * 3, 0);
            yuv.write_rgb8(&mut rgb);
            let mirror = shared.lock().unwrap().settings.mirror;
            normalize(&rgb, w, h, rotation, mirror, settings.size(), &mut rgba)?;
            let (tw, th) = settings.size();
            camera.send(&rgba, tw, th)?;
            let mut s = shared.lock().unwrap();
            s.frame.clear();
            s.frame.extend_from_slice(&rgba);
            s.size = (tw, th);
            s.sequence += 1;
            count += 1;
            if start.elapsed() >= Duration::from_secs(1) {
                s.status = format!(
                    "LIVE · {:.0} fps · {} · Rust / OpenH264",
                    count as f64 / start.elapsed().as_secs_f64(),
                    settings.resolution
                );
                start = Instant::now();
                count = 0;
            }
        }
    }
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}
fn local_ip() -> String {
    (|| -> std::io::Result<String> {
        let s = std::net::UdpSocket::bind("0.0.0.0:0")?;
        s.connect("192.0.2.1:9")?;
        Ok(s.local_addr()?.ip().to_string())
    })()
    .unwrap_or_else(|_| "127.0.0.1".into())
}

pub fn self_test() -> Result<serde_json::Value> {
    use camerabridge_core::write_packet;
    use openh264::{
        encoder::Encoder,
        formats::{RgbSliceU8, YUVBuffer},
    };
    let mut nonce = [0; 8];
    getrandom::getrandom(&mut nonce).map_err(|e| anyhow::anyhow!("Random: {e}"))?;
    let dir = std::env::temp_dir().join(format!(
        "CameraBridge-rust-test-{}-{}",
        std::process::id(),
        hex(&nonce)
    ));
    fs::create_dir_all(&dir)?;
    let suffix = format!("RustSelfTest{}", std::process::id());
    let receiver = Receiver::start_at(dir.clone(), "127.0.0.1:0", suffix.clone())?;
    let sample = Camera::new(&suffix)?;
    let pairing: serde_json::Value = serde_json::from_str(&receiver.pairing)?;
    let cert = rustls_pemfile::certs(&mut BufReader::new(fs::File::open(
        dir.join("identity/cert.pem"),
    )?))
    .next()
    .unwrap()?;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert)?;
    let config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );
    let raw = TcpStream::connect(("127.0.0.1", pairing["port"].as_u64().unwrap() as u16))?;
    raw.set_read_timeout(Some(Duration::from_secs(5)))?;
    let conn = rustls::ClientConnection::new(config, "CameraBridge".try_into()?)?;
    let mut tls = rustls::StreamOwned::new(conn, raw);
    write_json(
        &mut tls,
        &serde_json::json!({"v":2,"codec":"h264","remote_control":true,"token":pairing["token"]}),
    )?;
    let reply = read_json(&mut tls)?;
    if reply["width"] != 1280 || reply["height"] != 720 {
        bail!("Wrong negotiation");
    }
    let mut rgb = vec![0; 640 * 360 * 3];
    for y in 0..360 {
        for x in 0..640 {
            let i = (y * 640 + x) * 3;
            rgb[i..i + 3].copy_from_slice(if y < 180 {
                &[220, 30, 30]
            } else {
                &[30, 30, 220]
            });
        }
    }
    let yuv = YUVBuffer::from_rgb_source(RgbSliceU8::new(&rgb, (640, 360)));
    let mut encoder = Encoder::new()?;
    let packet = encoder.encode(&yuv)?.to_vec();
    let begin = Instant::now();
    for _ in 0..30 {
        write_packet(&mut tls, &packet, 0)?;
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        {
            let state = receiver.state.lock().unwrap();
            if state.sequence >= 30 {
                let top = &state.frame[..3];
                let bottom = &state.frame[(719 * 1280) * 4..(719 * 1280) * 4 + 3];
                if top[0] < 150 || top[2] > 100 || bottom[2] < 150 || bottom[0] > 100 {
                    bail!("Decoded orientation/color failed");
                }
                break;
            }
        }
        if Instant::now() > deadline {
            bail!(
                "Decode timed out: {}",
                receiver.state.lock().unwrap().status
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let elapsed = begin.elapsed().as_millis();
    // The first shared-memory row is the bottom of the displayed DirectShow image.
    let first = sample.pixel(0, 0)?;
    let last = sample.pixel(0, 719)?;
    if first[2] < 150 || first[0] > 100 || last[0] < 150 || last[2] > 100 {
        bail!("DirectShow bottom-up row order failed");
    }
    receiver.apply(
        Settings {
            resolution: "640x360".into(),
            ..Default::default()
        },
        false,
    )?;
    drop(tls);
    drop(receiver);
    drop(sample);
    let reopened = Receiver::start_at(dir.clone(), "127.0.0.1:0", suffix)?;
    let saved: serde_json::Value = serde_json::from_str(&reopened.pairing)?;
    if saved["token"] != pairing["token"] || saved["fingerprint"] != pairing["fingerprint"] {
        bail!("Identity did not persist");
    }
    if reopened.state.lock().unwrap().settings.resolution != "640x360" {
        bail!("Settings did not persist");
    }
    drop(reopened);
    // Only this process's explicitly named test directory is cleaned.
    fs::remove_dir_all(&dir)?;
    Ok(
        serde_json::json!({"ok":true,"frames":30,"elapsed_ms":elapsed,"tls":true,"identity_persistence":true,"settings_persistence":true,"upright_rgb":true,"directshow_bottom_up":true}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn tls_decode_publish_restart() {
        super::self_test().unwrap();
    }
}
