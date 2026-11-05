package app.camerabridge;

/** Pure stream policy so frame-rate selection can be tested without a phone. */
public final class StreamPolicy {
    private StreamPolicy() { }

    public static int chooseFpsRange(int[][] ranges, int requested, boolean steady) {
        int best = -1;
        long bestScore = Long.MAX_VALUE;
        for (int i = 0; i < ranges.length; i++) {
            int low = ranges[i][0], high = ranges[i][1];
            if (low <= 0 || high < low) continue;
            // Prefer a range containing the target. Fixed/high-floor ranges prevent
            // auto exposure silently falling to 5 fps in an otherwise 30-fps stream.
            long miss = Math.max(0, requested - high);
            long score = miss * 1000000L + Math.abs(high - requested) * 1000L
                         + (steady ? Math.max(0, requested - low) * 10L : low);
            if (score < bestScore) { bestScore = score; best = i; }
        }
        return best;
    }
}
