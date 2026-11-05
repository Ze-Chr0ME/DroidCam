package app.camerabridge;
// JVM fixture checks the exact Packet constructor ABI used by the Android adapter.
final class CameraEngine {
    static final class Packet {
        final byte[] data;
        final int rotation;
        Packet(byte[] data, int rotation) { this.data=data; this.rotation=rotation; }
    }
}
