package app.camerabridge;

/** JNI boundary for the Rust streaming core; no Android SDK types cross it. */
final class NativeBridge {
    static { System.loadLibrary("camerabridge_android"); }
    static native void run(CameraService service);
    static native int chooseFps(int[] ranges, int requested, int steady);
    static native long queueCreate();
    static native void queueReset(long queue);
    static native void queueDestroy(long queue);
    static native void queueConfig(long queue, byte[] bytes);
    static native boolean queuePush(long queue, byte[] bytes, int flags, int sensor, int display, boolean front);
    static native CameraEngine.Packet queuePoll(long queue, long timeout);
    static native int queueEncoded(long queue);
}
