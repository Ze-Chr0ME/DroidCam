import app.camerabridge.StreamPolicy;

public final class StreamPolicyTest {
    private static void expect(int expected, int actual, String label) {
        if (expected != actual) throw new AssertionError(label + ": expected " + expected + ", got " + actual);
    }
    public static void main(String[] args) {
        int[][] ranges = {{5,30}, {15,30}, {30,30}, {15,15}, {60,60}};
        expect(2, StreamPolicy.chooseFpsRange(ranges, 30, true), "Steady 30 must not pick 5-30 exposure");
        expect(0, StreamPolicy.chooseFpsRange(ranges, 30, false), "Low-light mode allows a variable range");
        expect(2, StreamPolicy.chooseFpsRange(ranges, 24, true), "24 fps encoder can use fixed 30 fps capture");
        expect(3, StreamPolicy.chooseFpsRange(ranges, 15, true), "15 fps saves capture work");
        expect(1, StreamPolicy.chooseFpsRange(new int[][]{{5,30},{15,30}}, 30, true), "Highest exposure floor if fixed mode unavailable");
        expect(0, StreamPolicy.chooseFpsRange(new int[][]{{15,30},{60,60}}, 30, true), "Do not use 60 fps for a 30 fps stream");
        expect(1, StreamPolicy.chooseFpsRange(new int[][]{{5,10},{15,15}}, 30, true), "Best available if target cannot be reached");
        expect(-1, StreamPolicy.chooseFpsRange(new int[][]{}, 30, true), "No ranges");
        expect(-1, StreamPolicy.chooseFpsRange(new int[][]{{0,30},{30,15}}, 30, true), "Reject invalid ranges");
        System.out.println("StreamPolicy: 9 frame-rate selection checks passed");
    }
}
