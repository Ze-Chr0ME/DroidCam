import app.camerabridge.PreviewGeometry;
import app.camerabridge.StreamOptions;
import java.util.Arrays;

public final class PhoneControlTest {
    private static void check(boolean condition, String message) {
        if (!condition) throw new AssertionError(message);
    }
    public static void main(String[] args) {
        check(PreviewGeometry.outputRotation(90,90,false) == 0, "Rear landscape output stays upright");
        check(PreviewGeometry.outputRotation(90,270,false) == 180, "Reverse landscape output");
        check(PreviewGeometry.outputRotation(270,90,true) == 0, "Front landscape output stays upright");
        check(PreviewGeometry.outputRotation(270,270,true) == 180, "Front reverse landscape");
        check(Arrays.equals(PreviewGeometry.fit(1280,720,90,90,false,1000,600), new int[]{1000,563}), "Landscape preview keeps aspect ratio");
        check(Arrays.equals(PreviewGeometry.fit(1280,720,90,270,false,1000,600), new int[]{1000,563}), "180 degree display flip does not stretch");
        check(Arrays.equals(PreviewGeometry.fit(1280,720,90,0,false,600,1000), new int[]{563,1000}), "Portrait fit is not transposed twice");
        StreamOptions defaults = StreamOptions.defaults();
        StreamOptions hidden = defaults.withPreview(false);
        check(hidden.enabled && !hidden.preview, "Disabling phone preview must keep capture enabled");
        check(hidden.withFront(true).enabled && hidden.withFront(true).front, "Lens control preserves capture");
        check(!hidden.withEnabled(false).enabled, "PC stop turns capture off");
        check(hidden.withEnabled(false).withEnabled(true).enabled, "PC restart needs no Activity");
        boolean rejected = false;
        try { new StreamOptions(8000,720,30,3000000,true,false,false,true,0); }
        catch (IllegalArgumentException expected) { rejected = true; }
        check(rejected, "Reject malformed remote settings");
        System.out.println("PhoneControl: 12 rotation, aspect-ratio and control checks passed");
    }
}
