package app.camerabridge;

import android.content.Context;
import android.view.SurfaceView;

/** A low-overhead, letterboxed viewfinder. Android owns its orientation transform. */
public final class CameraPreview extends SurfaceView {
    private int width = 1280, height = 720, sensor = 90, display = 90;
    private boolean front;

    public CameraPreview(Context context) { super(context); }

    public void cameraInfo(int width, int height, int sensor, boolean front, int display) {
        this.width = width; this.height = height; this.sensor = sensor;
        this.front = front; this.display = display;
        requestLayout();
    }

    @Override protected void onMeasure(int widthSpec, int heightSpec) {
        int[] fit = PreviewGeometry.fit(width, height, sensor, display, front,
                                        MeasureSpec.getSize(widthSpec), MeasureSpec.getSize(heightSpec));
        super.onMeasure(MeasureSpec.makeMeasureSpec(fit[0], MeasureSpec.EXACTLY),
                        MeasureSpec.makeMeasureSpec(fit[1], MeasureSpec.EXACTLY));
    }
}
