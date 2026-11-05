package app.camerabridge;

/** SurfaceView handles rotation itself; we supply aspect ratio, never a second matrix. */
public final class PreviewGeometry {
    private PreviewGeometry() { }

    public static int outputRotation(int sensor, int displayDegrees, boolean front) {
        return Math.floorMod(front ? sensor + displayDegrees : sensor - displayDegrees, 360);
    }

    public static int[] fit(int width, int height, int sensor, int displayDegrees,
                            boolean front, int availableWidth, int availableHeight) {
        if (width <= 0 || height <= 0 || availableWidth <= 0 || availableHeight <= 0)
            return new int[]{Math.max(1, availableWidth), Math.max(1, availableHeight)};
        if (outputRotation(sensor, displayDegrees, front) % 180 != 0) {
            int swap = width; width = height; height = swap;
        }
        double scale = Math.min((double)availableWidth / width, (double)availableHeight / height);
        return new int[]{Math.max(1, (int)Math.round(width * scale)),
                         Math.max(1, (int)Math.round(height * scale))};
    }
}
