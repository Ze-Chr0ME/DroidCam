package app.camerabridge;

/** Immutable camera configuration; also usable by the host-side Java tests. */
public final class StreamOptions {
    public final int width, height, fps, bitrate, revision;
    public final boolean steady, front, preview, enabled;

    public StreamOptions(int width, int height, int fps, int bitrate, boolean steady,
                         boolean front, boolean preview, boolean enabled, int revision) {
        if (width < 160 || width > 1280 || height < 90 || height > 720
            || width % 2 != 0 || height % 2 != 0 || fps < 10 || fps > 30
            || bitrate < 500000 || bitrate > 10000000 || revision < 0)
            throw new IllegalArgumentException("Invalid PC camera settings");
        this.width = width; this.height = height; this.fps = fps; this.bitrate = bitrate;
        this.steady = steady; this.front = front; this.preview = preview;
        this.enabled = enabled; this.revision = revision;
    }

    public static StreamOptions defaults() {
        return new StreamOptions(1280, 720, 30, 3000000, true, false, true, true, 0);
    }
    public StreamOptions withPreview(boolean value) {
        return new StreamOptions(width, height, fps, bitrate, steady, front, value, enabled, revision);
    }
    public StreamOptions withFront(boolean value) {
        return new StreamOptions(width, height, fps, bitrate, steady, value, preview, enabled, revision);
    }
    public StreamOptions withEnabled(boolean value) {
        return new StreamOptions(width, height, fps, bitrate, steady, front, preview, value, revision);
    }
}
