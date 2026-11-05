package app.camerabridge;

import android.Manifest;
import android.content.Context;
import android.content.pm.PackageManager;
import android.graphics.ImageFormat;
import android.hardware.camera2.*;
import android.hardware.camera2.params.StreamConfigurationMap;
import android.media.MediaCodec;
import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.media.MediaFormat;
import android.os.*;
import android.util.Range;
import android.util.Size;
import android.view.Surface;
import java.io.ByteArrayOutputStream;
import java.nio.ByteBuffer;
import java.util.*;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicInteger;

/** Camera and encoder belong to the service, never to an Activity or its window. */
final class CameraEngine {
    interface Listener {
        void info(int width, int height, int sensor, boolean front);
        void state(String message, boolean error);
    }
    static final class Packet {
        final byte[] data;
        final int rotation;
        Packet(byte[] data, int rotation) { this.data = data; this.rotation = rotation; }
    }

    private final Context context;
    private final Listener listener;
    private final HandlerThread thread = new HandlerThread("CameraBridge capture");
    private final Handler handler;
    private final long queue = NativeBridge.queueCreate();
    int takeEncoded() { return NativeBridge.queueEncoded(queue); }
    private CameraDevice camera;
    private CameraCaptureSession session;
    private MediaCodec encoder;
    private Surface input, preview;
    private StreamOptions options = StreamOptions.defaults();
    private boolean connected;
    private volatile int displayDegrees = 90;
    private int sensor = 90, generation;
    private int pendingOpens;
    private volatile boolean closed;
    private boolean shutdownClosed;
    volatile boolean active;

    CameraEngine(Context context, Listener listener) {
        this.context = context.getApplicationContext(); this.listener = listener;
        thread.start(); handler = new Handler(thread.getLooper());
    }

    void apply(StreamOptions value, boolean connected) {
        if (closed) return;
        handler.post(() -> { options = value; this.connected = connected; rebuild(); });
    }

    void preview(Surface surface, int degrees) {
        if (closed) return;
        handler.post(() -> {
            displayDegrees = degrees;
            if (preview == surface) return;
            preview = surface;
            if (options.preview && connected && options.enabled) rebuild();
        });
    }

    void rotation(int degrees) { displayDegrees = degrees; }
    Packet poll(long milliseconds) throws InterruptedException { return NativeBridge.queuePoll(queue, milliseconds); }

    private void rebuild() {
        if (closed) return;
        closeCamera();
        if (!connected || !options.enabled) return;
        if (context.checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED) {
            listener.state("Camera permission is missing. Open CameraBridge to grant it.", true); return;
        }
        final int current = generation;
        final StreamOptions settings = options;
        try {
            CameraManager manager = (CameraManager)context.getSystemService(Context.CAMERA_SERVICE);
            String id = null;
            CameraCharacteristics info = null;
            int facing = settings.front ? CameraCharacteristics.LENS_FACING_FRONT : CameraCharacteristics.LENS_FACING_BACK;
            for (String candidate : manager.getCameraIdList()) {
                CameraCharacteristics value = manager.getCameraCharacteristics(candidate);
                if (Objects.equals(value.get(CameraCharacteristics.LENS_FACING), facing)) { id = candidate; info = value; break; }
            }
            if (id == null) throw new Exception("Requested camera unavailable");
            sensor = info.get(CameraCharacteristics.SENSOR_ORIENTATION);
            StreamConfigurationMap map = info.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP);
            Size[] candidates = map.getOutputSizes(MediaCodec.class);
            if (candidates == null) candidates = map.getOutputSizes(ImageFormat.YUV_420_888);
            double aspect = (double)settings.width / settings.height;
            Size size = Arrays.stream(candidates).filter(s -> s.getWidth() <= 1280 && s.getHeight() <= 720)
                .min(Comparator.comparingDouble(s -> Math.abs((double)s.getWidth()/s.getHeight() - aspect) * 10000
                                                   + Math.abs(s.getWidth() - settings.width)))
                .orElseThrow(() -> new Exception("No compatible camera size"));
            listener.info(size.getWidth(), size.getHeight(), sensor, settings.front);
            createEncoder(size, current, settings);
            final List<Surface> outputs = new ArrayList<>();
            outputs.add(input);
            if (settings.preview && preview != null && preview.isValid()) outputs.add(preview);
            final Range<Integer>[] fps = info.get(CameraCharacteristics.CONTROL_AE_AVAILABLE_TARGET_FPS_RANGES);
            pendingOpens++;
            try { manager.openCamera(id, new CameraDevice.StateCallback() {
                private boolean finished;
                private void finishOpen() {
                    if (!finished) { finished = true; pendingOpens--; finishShutdown(); }
                }
                @Override public void onOpened(CameraDevice opened) {
                    finishOpen();
                    if (closed || current != generation) { opened.close(); return; }
                    camera = opened;
                    try {
                        opened.createCaptureSession(outputs, new CameraCaptureSession.StateCallback() {
                            @Override public void onConfigured(CameraCaptureSession configured) {
                                if (current != generation) { configured.close(); return; }
                                session = configured;
                                try {
                                    CaptureRequest.Builder request = opened.createCaptureRequest(CameraDevice.TEMPLATE_RECORD);
                                    for (Surface surface : outputs) request.addTarget(surface);
                                    request.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_VIDEO);
                                    int[][] ranges = new int[fps.length][2];
                                    for (int i = 0; i < fps.length; i++) { ranges[i][0] = fps[i].getLower(); ranges[i][1] = fps[i].getUpper(); }
                                    int[] flat = new int[ranges.length * 2];
                                    for (int i = 0; i < ranges.length; i++) { flat[i*2] = ranges[i][0]; flat[i*2+1] = ranges[i][1]; }
                                    int choice = NativeBridge.chooseFps(flat, settings.fps, settings.steady ? 1 : 0);
                                    if (choice >= 0) request.set(CaptureRequest.CONTROL_AE_TARGET_FPS_RANGE, fps[choice]);
                                    configured.setRepeatingRequest(request.build(), null, handler);
                                    active = true;
                                    listener.state("Camera live; screen may be locked", false);
                                } catch (Exception error) { fail(current, error); }
                            }
                            @Override public void onConfigureFailed(CameraCaptureSession failed) {
                                fail(current, new Exception("Camera session could not start"));
                            }
                        }, handler);
                    } catch (Exception error) { fail(current, error); }
                }
                @Override public void onDisconnected(CameraDevice opened) {
                    finishOpen();
                    opened.close(); fail(current, new Exception("Camera was interrupted by another app"));
                }
                @Override public void onError(CameraDevice opened, int code) {
                    finishOpen();
                    opened.close(); fail(current, new Exception("Camera error " + code));
                }
            }, handler); } catch (Exception error) { pendingOpens--; throw error; }
        } catch (Exception error) { fail(current, error); }
    }

    private void fail(int current, Exception error) {
        if (closed || current != generation) return;
        closeCamera();
        listener.state(error.getMessage() == null ? "Camera unavailable" : error.getMessage(), true);
        // Foreground service retains access; retry transient camera preemption without a phone tap.
        final int retryGeneration = generation;
        handler.postDelayed(() -> { if (generation == retryGeneration && connected && options.enabled) rebuild(); }, 2000);
    }

    private void createEncoder(Size size, final int current, final StreamOptions settings) throws Exception {
        MediaFormat format = MediaFormat.createVideoFormat("video/avc", size.getWidth(), size.getHeight());
        format.setInteger(MediaFormat.KEY_COLOR_FORMAT, MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface);
        format.setInteger(MediaFormat.KEY_BIT_RATE, settings.bitrate);
        format.setInteger(MediaFormat.KEY_FRAME_RATE, settings.fps);
        format.setInteger(MediaFormat.KEY_I_FRAME_INTERVAL, 1);
        format.setInteger(MediaFormat.KEY_PROFILE, MediaCodecInfo.CodecProfileLevel.AVCProfileBaseline);
        format.setInteger(MediaFormat.KEY_PRIORITY, 0);
        if (Build.VERSION.SDK_INT >= 29) {
            format.setFloat(MediaFormat.KEY_MAX_FPS_TO_ENCODER, (float)settings.fps);
            format.setInteger(MediaFormat.KEY_MAX_B_FRAMES, 0);
        }
        // isFormatSupported() rejects optional keys on many vendor encoders; rank by size, let configure() decide.
        List<String> names = new ArrayList<>(), software = new ArrayList<>();
        for (MediaCodecInfo codec : new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos()) {
            if (!codec.isEncoder()) continue;
            boolean hardware = Build.VERSION.SDK_INT >= 29 ? codec.isHardwareAccelerated()
                : !(codec.getName().startsWith("OMX.google.") || codec.getName().startsWith("c2.android."));
            for (String type : codec.getSupportedTypes()) {
                if (!type.equalsIgnoreCase("video/avc")) continue;
                MediaCodecInfo.VideoCapabilities video = codec.getCapabilitiesForType(type).getVideoCapabilities();
                if (video != null && video.isSizeSupported(size.getWidth(), size.getHeight())) {
                    (hardware ? names : software).add(codec.getName()); break;
                }
            }
        }
        names.addAll(software);
        MediaFormat minimal = MediaFormat.createVideoFormat("video/avc", size.getWidth(), size.getHeight());
        for (String key : new String[]{MediaFormat.KEY_COLOR_FORMAT, MediaFormat.KEY_BIT_RATE, MediaFormat.KEY_FRAME_RATE, MediaFormat.KEY_I_FRAME_INTERVAL})
            minimal.setInteger(key, format.getInteger(key));
        MediaCodec.Callback callback = new MediaCodec.Callback() {
            @Override public void onInputBufferAvailable(MediaCodec codec, int index) { }
            @Override public void onOutputFormatChanged(MediaCodec codec, MediaFormat output) {
                if (current != generation) return;
                ByteArrayOutputStream bytes = new ByteArrayOutputStream();
                for (String key : new String[]{"csd-0", "csd-1"}) {
                    ByteBuffer value = output.getByteBuffer(key);
                    if (value != null) {
                        byte[] part = new byte[value.remaining()]; value.duplicate().get(part); bytes.write(part, 0, part.length);
                    }
                }
                NativeBridge.queueConfig(queue, bytes.toByteArray());
            }
            @Override public void onOutputBufferAvailable(MediaCodec codec, int index, MediaCodec.BufferInfo info) {
                try {
                    if (current != generation || !connected) return;
                    ByteBuffer buffer = codec.getOutputBuffer(index);
                    if (buffer == null || info.size <= 0) return;
                    buffer.position(info.offset); buffer.limit(info.offset + info.size);
                    byte[] data = new byte[info.size]; buffer.get(data);
                    if (NativeBridge.queuePush(queue, data, info.flags, sensor, displayDegrees, settings.front)) {
                        Bundle parameters = new Bundle();
                        parameters.putInt(MediaCodec.PARAMETER_KEY_REQUEST_SYNC_FRAME, 0);
                        codec.setParameters(parameters);
                    }
                } catch (IllegalStateException ignored) {
                } finally { try { codec.releaseOutputBuffer(index, false); } catch (IllegalStateException ignored) { } }
            }
            @Override public void onError(MediaCodec codec, MediaCodec.CodecException error) { fail(current, error); }
        };
        Exception last = null;
        search:
        for (String name : names) {
            for (MediaFormat attempt : new MediaFormat[]{format, minimal}) {
                MediaCodec candidate = null;
                try {
                    candidate = MediaCodec.createByCodecName(name);
                    candidate.setCallback(callback, handler);
                    candidate.configure(attempt, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE);
                    encoder = candidate; break search;
                } catch (Exception error) {
                    last = error;
                    if (candidate != null) candidate.release();
                }
            }
        }
        if (encoder == null) throw new Exception("No compatible H.264 encoder for " + size.getWidth() + "x" + size.getHeight()
            + (last == null ? "" : " (" + last.getMessage() + ")"));
        input = encoder.createInputSurface();
        encoder.start();
    }

    private void closeCamera() {
        generation++; active = false;
        if (session != null) { session.close(); session = null; }
        if (camera != null) { camera.close(); camera = null; }
        if (encoder != null) {
            try { encoder.stop(); } catch (IllegalStateException ignored) { }
            encoder.release(); encoder = null;
        }
        if (input != null) { input.release(); input = null; }
        NativeBridge.queueReset(queue);
    }

    void shutdown() {
        closed = true;
        handler.post(() -> { connected = false; preview = null; closeCamera(); shutdownClosed = true; finishShutdown(); });
    }
    private boolean queueDestroyed;
    private void finishShutdown() {
        if (closed && shutdownClosed && pendingOpens == 0 && !queueDestroyed) {
            queueDestroyed = true; NativeBridge.queueDestroy(queue); thread.quitSafely();
        }
    }
}
