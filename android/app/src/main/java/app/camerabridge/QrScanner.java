package app.camerabridge;

import android.Manifest;
import android.content.Context;
import android.content.pm.PackageManager;
import android.graphics.ImageFormat;
import android.hardware.camera2.*;
import android.media.Image;
import android.media.ImageReader;
import android.os.*;
import android.view.Surface;
import com.google.zxing.*;
import com.google.zxing.common.HybridBinarizer;
import java.nio.ByteBuffer;
import java.util.Arrays;
import java.util.Collections;

/** Pairing-only low-resolution scanner; the service owns streaming. */
final class QrScanner {
    interface Listener { void found(String text); void error(String message); void info(int sensor); }
    private final Context context;
    private final Listener listener;
    private final HandlerThread thread = new HandlerThread("CameraBridge QR");
    private final Handler handler;
    private final MultiFormatReader decoder = new MultiFormatReader();
    private volatile boolean stopped;
    private CameraDevice camera;
    private CameraCaptureSession session;
    private ImageReader reader;
    private long last;

    QrScanner(Context context, Surface surface, Listener listener) {
        this.context = context.getApplicationContext(); this.listener = listener;
        decoder.setHints(Collections.singletonMap(DecodeHintType.POSSIBLE_FORMATS,
                                                   Collections.singletonList(BarcodeFormat.QR_CODE)));
        thread.start(); handler = new Handler(thread.getLooper());
        handler.postDelayed(() -> open(surface), 250);
    }
    private void open(Surface surface) {
        if (stopped || !surface.isValid()) return;
        try {
            if (context.checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED)
                throw new Exception("Allow camera permission to scan");
            CameraManager manager = (CameraManager)context.getSystemService(Context.CAMERA_SERVICE);
            String rear = null;
            for (String id : manager.getCameraIdList()) {
                CameraCharacteristics info = manager.getCameraCharacteristics(id);
                if (info.get(CameraCharacteristics.LENS_FACING) == CameraCharacteristics.LENS_FACING_BACK) {
                    rear = id; listener.info(info.get(CameraCharacteristics.SENSOR_ORIENTATION)); break;
                }
            }
            if (rear == null) throw new Exception("Rear camera unavailable");
            reader = ImageReader.newInstance(640, 480, ImageFormat.YUV_420_888, 3);
            reader.setOnImageAvailableListener(this::image, handler);
            manager.openCamera(rear, new CameraDevice.StateCallback() {
                @Override public void onOpened(CameraDevice opened) {
                    if (stopped) { opened.close(); return; }
                    camera = opened;
                    try {
                        opened.createCaptureSession(Arrays.asList(surface, reader.getSurface()), new CameraCaptureSession.StateCallback() {
                            @Override public void onConfigured(CameraCaptureSession configured) {
                                if (stopped) { configured.close(); return; }
                                session = configured;
                                try {
                                    CaptureRequest.Builder request = opened.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW);
                                    request.addTarget(surface); request.addTarget(reader.getSurface());
                                    request.set(CaptureRequest.CONTROL_AF_MODE, CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE);
                                    configured.setRepeatingRequest(request.build(), null, handler);
                                } catch (Exception error) { listener.error(error.getMessage()); }
                            }
                            @Override public void onConfigureFailed(CameraCaptureSession failed) { listener.error("QR camera could not start"); }
                        }, handler);
                    } catch (Exception error) { listener.error(error.getMessage()); }
                }
                @Override public void onDisconnected(CameraDevice opened) { opened.close(); }
                @Override public void onError(CameraDevice opened, int error) { opened.close(); listener.error("QR camera unavailable; try scanning again"); }
            }, handler);
        } catch (Exception error) { if (!stopped) listener.error(error.getMessage()); }
    }
    private void image(ImageReader source) {
        try (Image image = source.acquireLatestImage()) {
            if (image == null || stopped || SystemClock.elapsedRealtime() - last < 180) return;
            last = SystemClock.elapsedRealtime();
            int width = image.getWidth(), height = image.getHeight();
            Image.Plane plane = image.getPlanes()[0]; ByteBuffer buffer = plane.getBuffer();
            int base = buffer.position(), stride = plane.getRowStride(), pixel = plane.getPixelStride();
            byte[] luma = new byte[width * height];
            for (int y = 0; y < height; y++) {
                if (pixel == 1) { buffer.position(base + y * stride); buffer.get(luma, y * width, width); }
                else for (int x = 0; x < width; x++) luma[y * width + x] = buffer.get(base + y * stride + x * pixel);
            }
            try {
                Result result = decoder.decodeWithState(new BinaryBitmap(new HybridBinarizer(
                    new PlanarYUVLuminanceSource(luma, width, height, 0, 0, width, height, false))));
                if (!stopped) { stopped = true; listener.found(result.getText()); }
            } catch (NotFoundException ignored) { } finally { decoder.reset(); }
        } catch (IllegalStateException ignored) { }
    }
    void stop() {
        stopped = true;
        handler.post(() -> {
            if (session != null) session.close();
            if (camera != null) camera.close();
            if (reader != null) reader.close();
            handler.postDelayed(thread::quitSafely, 1000);
        });
    }
}
