package app.camerabridge;

import android.Manifest;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.content.res.ColorStateList;
import android.content.res.Configuration;
import android.graphics.Typeface;
import android.graphics.drawable.ColorDrawable;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.hardware.display.DisplayManager;
import android.os.*;
import android.provider.Settings;
import android.text.TextUtils;
import android.view.*;
import android.widget.*;

/** Optional phone UI; CameraService keeps capture alive independently of this window. */
public final class MainActivity extends Activity implements CameraService.Listener, DisplayManager.DisplayListener {
    private CameraPreview preview;
    private TextView status, chip, placeholderTitle, placeholderBody, lensValue, footnote;
    private View placeholder, previewRow, lensRow, stopButton;
    private TextView controlButton;
    private Switch previewSwitch;
    private CameraService service;
    private boolean bound, visible, scanning, streaming, phonePreview = true;
    private boolean pendingEnable, pendingScan;
    private String pendingPairing;
    private QrScanner scanner;
    private int captureWidth = 1280, captureHeight = 720, sensor = 90;
    private boolean front;
    private final Handler main = new Handler(Looper.getMainLooper());

    private static final int BG = 0xff0f1012, STAGE = 0xff000000, CARD = 0xff1b1d21, LINE = 0xff2a2d32;
    private static final int TEXT = 0xffeceef0, MUTED = 0xff8e939b, ACCENT = 0xff5b8cff;
    private static final int GREEN = 0xff3fb97a, AMBER = 0xffe0a84a, RED = 0xffef6b62;
    private static final Typeface MEDIUM = Typeface.create("sans-serif-medium", Typeface.NORMAL);

    private final ServiceConnection connection = new ServiceConnection() {
        @Override public void onServiceConnected(ComponentName name, IBinder binder) {
            if (!bound || !visible) return;
            service = ((CameraService.LocalBinder)binder).service();
            if (visible) { service.listen(MainActivity.this); attachPreview(); }
            main.postDelayed(() -> {
                if (visible && CameraService.available && !getSharedPreferences("camera", MODE_PRIVATE).getBoolean("batteryAsked", false)) {
                    getSharedPreferences("camera", MODE_PRIVATE).edit().putBoolean("batteryAsked", true).apply();
                    PowerManager power = (PowerManager)getSystemService(POWER_SERVICE);
                    if (!power.isIgnoringBatteryOptimizations(getPackageName())) batterySettings();
                }
            }, 600);
        }
        @Override public void onServiceDisconnected(ComponentName name) {
            service = null; state("PC control stopped. Enable it again to reconnect.", false, false, false);
        }
    };

    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        edgeToEdge();
        LinearLayout root = new LinearLayout(this); root.setBackgroundColor(BG);
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            int[] bars = systemBars(insets);
            view.setPadding(bars[0], bars[1], bars[2], bars[3]);
            return insets;
        });

        // Viewfinder
        FrameLayout stage = new FrameLayout(this);
        stage.setBackground(round(STAGE, 16, 0));
        stage.setClipToOutline(true);
        preview = new CameraPreview(this);
        stage.addView(preview, new FrameLayout.LayoutParams(-1, -1, Gravity.CENTER));
        LinearLayout empty = new LinearLayout(this); empty.setOrientation(LinearLayout.VERTICAL);
        empty.setGravity(Gravity.CENTER); empty.setPadding(dp(32), 0, dp(32), 0);
        placeholderTitle = text("", 17, TEXT, true); placeholderTitle.setGravity(Gravity.CENTER);
        placeholderBody = text("", 13, MUTED, false); placeholderBody.setGravity(Gravity.CENTER);
        placeholderBody.setMaxWidth(dp(340)); placeholderBody.setPadding(0, dp(6), 0, 0);
        placeholderBody.setLineSpacing(dp(2), 1);
        empty.addView(placeholderTitle); empty.addView(placeholderBody);
        placeholder = empty;
        stage.addView(empty, new FrameLayout.LayoutParams(-1, -1));
        chip = text("", 12, TEXT, true);
        chip.setBackground(round(0xb3000000, 999, 0));
        chip.setPadding(dp(10), dp(5), dp(12), dp(5));
        chip.setCompoundDrawablePadding(dp(7));
        chip.setGravity(Gravity.CENTER_VERTICAL);
        FrameLayout.LayoutParams chipAt = new FrameLayout.LayoutParams(-2, -2, Gravity.TOP | Gravity.START);
        chipAt.setMargins(dp(12), dp(12), 0, 0);
        stage.addView(chip, chipAt);
        LinearLayout.LayoutParams stageAt = new LinearLayout.LayoutParams(0, -1, 1);
        stageAt.setMargins(dp(10), dp(10), dp(4), dp(10));
        root.addView(stage, stageAt);

        // Control panel
        ScrollView scroll = new ScrollView(this); scroll.setFillViewport(true);
        scroll.setVerticalScrollBarEnabled(false); scroll.setOverScrollMode(View.OVER_SCROLL_NEVER);
        LinearLayout panel = new LinearLayout(this); panel.setOrientation(LinearLayout.VERTICAL);
        panel.setPadding(dp(14), dp(16), dp(18), dp(16)); scroll.addView(panel);
        root.addView(scroll, new LinearLayout.LayoutParams(dp(312), -1));

        panel.addView(text("CameraBridge", 20, TEXT, true));
        status = text("Pair once, then control the webcam from your PC.", 13, MUTED, false);
        status.setPadding(0, dp(4), 0, 0); status.setMaxLines(3); status.setEllipsize(TextUtils.TruncateAt.END);
        panel.addView(status);

        controlButton = text("", 15, 0xffffffff, true);
        controlButton.setGravity(Gravity.CENTER); controlButton.setClickable(true); controlButton.setFocusable(true);
        controlButton.setOnClickListener(v -> primaryAction());
        LinearLayout.LayoutParams primaryAt = new LinearLayout.LayoutParams(-1, dp(50));
        primaryAt.setMargins(0, dp(16), 0, 0);
        panel.addView(controlButton, primaryAt);

        section(panel, "Camera");
        LinearLayout camera = card(panel);
        previewSwitch = new Switch(this);
        previewSwitch.setThumbTintList(states(0xffffffff, 0xffc4c7cc));
        previewSwitch.setTrackTintList(states(ACCENT, 0xff3a3d43));
        previewSwitch.setClickable(false); previewSwitch.setFocusable(false);
        previewRow = row(camera, "Phone preview", previewSwitch, v -> {
            if (service == null) return;
            service.setPhonePreview(!previewSwitch.isChecked());
        });
        divider(camera);
        lensValue = text("", 14, ACCENT, false);
        lensRow = row(camera, "Lens", lensValue, v -> { if (service != null) service.switchLens(); });

        section(panel, "Pairing");
        LinearLayout pairing = card(panel);
        row(pairing, "Scan PC pairing code", chevron(), v -> beginScan());
        divider(pairing);
        row(pairing, "Paste pairing text", chevron(), v -> pasteDialog());

        section(panel, "Background");
        LinearLayout background = card(panel);
        row(background, "Battery & screen-off", chevron(), v -> batterySettings());

        TextView stop = text("Stop sharing", 14, RED, true);
        stop.setGravity(Gravity.CENTER); stop.setClickable(true); stop.setFocusable(true);
        stop.setBackground(ripple(round(0x00000000, 12, RED & 0x66ffffff)));
        stop.setOnClickListener(v -> disarm());
        LinearLayout.LayoutParams stopAt = new LinearLayout.LayoutParams(-1, dp(44));
        stopAt.setMargins(0, dp(16), 0, 0);
        panel.addView(stop, stopAt);
        stopButton = stop;

        footnote = text("Keeps streaming while you switch apps or lock the screen. Swipe CameraBridge away from Recents to stop completely.", 12, MUTED, false);
        footnote.setPadding(dp(2), dp(14), dp(2), 0); footnote.setLineSpacing(dp(2), 1);
        panel.addView(footnote);

        setContentView(root);
        preview.getHolder().addCallback(new SurfaceHolder.Callback() {
            public void surfaceCreated(SurfaceHolder holder) { if (scanning) startScanner(); else attachPreview(); }
            public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) { attachPreview(); }
            public void surfaceDestroyed(SurfaceHolder holder) {
                stopScanner(); if (service != null) service.preview(null, displayDegrees());
            }
        });
        showPreview(false);
        refresh();
    }

    // ---- Styling helpers -------------------------------------------------------------------

    private int dp(int value) { return (int)(value * getResources().getDisplayMetrics().density); }
    private void edgeToEdge() {
        Window window = getWindow();
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false);
            window.setStatusBarColor(0); window.setNavigationBarColor(0);
        } else { window.setStatusBarColor(BG); window.setNavigationBarColor(BG); }
    }
    @SuppressWarnings("deprecation")
    private static int[] systemBars(WindowInsets insets) {
        if (Build.VERSION.SDK_INT >= 30) {
            android.graphics.Insets i = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
            return new int[]{i.left, i.top, i.right, i.bottom};
        }
        return new int[]{insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(),
                         insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom()};
    }
    private GradientDrawable round(int fill, int radius, int stroke) {
        GradientDrawable shape = new GradientDrawable();
        shape.setColor(fill); shape.setCornerRadius(dp(radius));
        if (stroke != 0) shape.setStroke(Math.max(1, dp(1)), stroke);
        return shape;
    }
    private Drawable ripple(Drawable content) {
        return new RippleDrawable(ColorStateList.valueOf(0x24ffffff), content, content == null ? new ColorDrawable(0xffffffff) : null);
    }
    private static ColorStateList states(int on, int off) {
        return new ColorStateList(new int[][]{{android.R.attr.state_checked}, {}}, new int[]{on, off});
    }
    private TextView text(String value, int size, int color, boolean medium) {
        TextView view = new TextView(this); view.setText(value); view.setTextSize(size); view.setTextColor(color);
        view.setTypeface(medium ? MEDIUM : Typeface.DEFAULT); view.setIncludeFontPadding(true);
        return view;
    }
    private void section(LinearLayout parent, String title) {
        TextView label = text(title.toUpperCase(), 11, MUTED, true);
        label.setLetterSpacing(0.08f); label.setPadding(dp(4), dp(20), 0, dp(8));
        parent.addView(label);
    }
    private LinearLayout card(LinearLayout parent) {
        LinearLayout card = new LinearLayout(this); card.setOrientation(LinearLayout.VERTICAL);
        card.setBackground(round(CARD, 14, 0)); card.setClipToOutline(true);
        parent.addView(card, new LinearLayout.LayoutParams(-1, -2));
        return card;
    }
    private void divider(LinearLayout card) {
        View line = new View(this); line.setBackgroundColor(LINE);
        LinearLayout.LayoutParams at = new LinearLayout.LayoutParams(-1, Math.max(1, dp(1) / 2 + 1));
        at.setMargins(dp(14), 0, 0, 0);
        card.addView(line, at);
    }
    private View row(LinearLayout card, String title, View trailing, View.OnClickListener click) {
        LinearLayout row = new LinearLayout(this);
        row.setGravity(Gravity.CENTER_VERTICAL); row.setMinimumHeight(dp(50));
        row.setPadding(dp(14), 0, dp(12), 0);
        row.setBackground(ripple(null)); row.setClickable(true); row.setFocusable(true);
        row.setOnClickListener(click);
        row.addView(text(title, 14, TEXT, false), new LinearLayout.LayoutParams(0, -2, 1));
        row.addView(trailing, new LinearLayout.LayoutParams(-2, -2));
        card.addView(row, new LinearLayout.LayoutParams(-1, -2));
        return row;
    }
    private TextView chevron() { return text("›", 20, MUTED, false); }
    private Drawable dot(int color) {
        GradientDrawable dot = new GradientDrawable(); dot.setShape(GradientDrawable.OVAL);
        dot.setColor(color); dot.setSize(dp(8), dp(8)); return dot;
    }
    private static void enable(View view, boolean on) {
        view.setEnabled(on); view.setAlpha(on ? 1f : 0.4f);
    }

    // ---- State rendering -------------------------------------------------------------------

    private boolean paired() { return getSharedPreferences("camera", MODE_PRIVATE).getString("pairing", null) != null; }
    private void primaryAction() {
        if (scanning) { cancelScan(); return; }
        if (service != null && CameraService.available) service.setStreaming(!streaming);
        else if (paired()) enableControl(null);
        else beginScan();
    }
    private void refresh() {
        boolean sharing = CameraService.available;
        boolean connected = sharing && service != null && service.connected();
        String label; boolean primary = true;
        if (scanning) { label = "Cancel scan"; primary = false; }
        else if (!sharing) label = paired() ? "Enable PC control" : "Scan PC pairing code";
        else if (streaming) { label = "Stop camera"; primary = false; }
        else label = "Start camera";
        controlButton.setText(label);
        controlButton.setTextColor(primary ? 0xffffffff : TEXT);
        controlButton.setBackground(ripple(round(primary ? ACCENT : CARD, 12, primary ? 0 : LINE)));

        previewSwitch.setChecked(phonePreview);
        lensValue.setText(front ? "Front · switch" : "Rear · switch");
        enable(previewRow, sharing && service != null && !scanning);
        enable(lensRow, sharing && service != null && !scanning);
        stopButton.setVisibility(sharing ? View.VISIBLE : View.GONE);

        String chipText; int chipColor;
        if (scanning) { chipText = "Scanning"; chipColor = AMBER; }
        else if (!sharing) { chipText = "Off"; chipColor = MUTED; }
        else if (streaming) { chipText = "Live"; chipColor = RED; }
        else if (connected) { chipText = "Connected"; chipColor = GREEN; }
        else { chipText = "Waiting for PC"; chipColor = AMBER; }
        chip.setText(chipText);
        chip.setCompoundDrawablesRelativeWithIntrinsicBounds(dot(chipColor), null, null, null);

        String title, body;
        if (!paired()) { title = "Not paired"; body = "Open CameraBridge on your PC and scan the pairing code it shows."; }
        else if (!sharing) { title = "PC control is off"; body = "Enable PC control so your computer can start the camera."; }
        else if (!connected) { title = "Waiting for your PC"; body = "Open CameraBridge on the PC. Both devices need to be on the same Wi-Fi network."; }
        else if (!streaming) { title = "Connected"; body = "Start the camera here or from the PC."; }
        else { title = "Preview off"; body = "Your PC is still receiving video."; }
        placeholderTitle.setText(title); placeholderBody.setText(body);
    }

    // ---- Behavior --------------------------------------------------------------------------

    private void pasteDialog() {
        EditText text = new EditText(this); text.setHint("Pairing text from the PC");
        text.setSingleLine(false); text.setMaxLines(4);
        FrameLayout box = new FrameLayout(this); box.setPadding(dp(20), dp(8), dp(20), 0);
        box.addView(text);
        new AlertDialog.Builder(this).setTitle("Paste pairing text").setView(box)
            .setPositiveButton("Pair", (d, w) -> pair(text.getText().toString()))
            .setNegativeButton("Cancel", null).show();
    }
    private void batterySettings() {
        PowerManager power = (PowerManager)getSystemService(POWER_SERVICE);
        android.net.Uri app = android.net.Uri.parse("package:" + getPackageName());
        String action = power.isIgnoringBatteryOptimizations(getPackageName())
            ? Settings.ACTION_APPLICATION_DETAILS_SETTINGS : Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS;
        status.setText("Allow background activity so the PC can use the camera with the screen off. On Redmi, also choose No restrictions.");
        try { startActivity(new Intent(action, app)); }
        catch (ActivityNotFoundException unavailable) { startActivity(new Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, app)); }
    }

    @Override public void onResume() {
        super.onResume(); visible = true;
        ((DisplayManager)getSystemService(DISPLAY_SERVICE)).registerDisplayListener(this, main);
        if (CameraService.available) bindControl();
        else if (pendingEnable && cameraAllowed()) {
            String pairing = pendingPairing; pendingEnable = false; enableControl(pairing);
        }
        else if (getSharedPreferences("camera", MODE_PRIVATE).getBoolean("wanted", false) && cameraAllowed()) enableControl(null);
        refresh();
    }
    @Override public void onPause() {
        visible = false; scanning = false; stopScanner();
        getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        ((DisplayManager)getSystemService(DISPLAY_SERVICE)).unregisterDisplayListener(this);
        detachControl();
        // The foreground service, including encoder-only capture, remains running.
        super.onPause();
    }
    private boolean cameraAllowed() { return checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED; }
    private boolean askPermission() {
        if (cameraAllowed()) return true;
        if (Build.VERSION.SDK_INT >= 33)
            getSharedPreferences("camera", MODE_PRIVATE).edit().putBoolean("notificationAsked", true).apply();
        requestPermissions(Build.VERSION.SDK_INT >= 33
            ? new String[]{Manifest.permission.CAMERA, Manifest.permission.POST_NOTIFICATIONS}
            : new String[]{Manifest.permission.CAMERA}, 4);
        status.setText("Allow camera access, then enable PC control or scan again."); return false;
    }
    private void bindControl() { if (!bound) bound = bindService(new Intent(this, CameraService.class), connection, 0); }
    private void detachControl() {
        if (service != null) { service.preview(null, displayDegrees()); service.listen(null); }
        if (bound) { unbindService(connection); bound = false; }
        service = null;
    }
    private void enableControl(String pairing) {
        if (!visible) { pendingEnable = true; pendingPairing = pairing; return; }
        if (!cameraAllowed()) {
            pendingEnable = true; pendingPairing = pairing;
            askPermission(); return;
        }
        if (pairing == null) pairing = getSharedPreferences("camera", MODE_PRIVATE).getString("pairing", null);
        if (pairing == null) { status.setText("Scan the code on the PC first."); return; }
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            && !getSharedPreferences("camera", MODE_PRIVATE).getBoolean("notificationAsked", false)) {
            pendingEnable = true; pendingPairing = pairing;
            getSharedPreferences("camera", MODE_PRIVATE).edit().putBoolean("notificationAsked", true).apply();
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, 5);
            return;
        }
        try {
            CameraService.validatePairing(pairing); scanning = false; stopScanner();
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
            getSharedPreferences("camera", MODE_PRIVATE).edit().putBoolean("wanted", true).apply();
            startForegroundService(new Intent(this, CameraService.class).setAction(CameraService.ARM).putExtra("pairing", pairing));
            if (!bound) bound = bindService(new Intent(this, CameraService.class), connection, Context.BIND_AUTO_CREATE);
            status.setText("PC control enabled. You can lock the screen now.");
            showPreview(false); refresh();
        } catch (Exception error) { status.setText("Could not enable PC control: " + error.getMessage()); }
    }
    private void pair(String text) {
        try { CameraService.validatePairing(text); enableControl(text); }
        catch (Exception error) { status.setText("That isn't a valid pairing code. Scan the code shown in the PC app."); }
    }
    private void disarm() {
        scanning = false; stopScanner();
        getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        getSharedPreferences("camera", MODE_PRIVATE).edit().putBoolean("wanted", false).apply();
        if (service != null) service.stopSharing();
        else if (CameraService.available) startService(new Intent(this, CameraService.class).setAction(CameraService.STOP));
        detachControl(); showPreview(false); streaming = false;
        status.setText("Sharing stopped. The PC can no longer start the camera.");
        refresh();
    }
    private void beginScan() {
        if (!cameraAllowed()) { pendingScan = true; askPermission(); return; }
        disarm(); scanning = true; getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        captureWidth = 640; captureHeight = 480; sensor = 90; front = false;
        updateAspect(); preview.getHolder().setFixedSize(640,480); showPreview(true);
        status.setText("Point the rear camera at the code on your PC."); startScanner();
        refresh();
    }
    private void cancelScan() {
        scanning = false; stopScanner();
        getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        showPreview(false);
        status.setText("Scan cancelled.");
        refresh();
    }
    private void startScanner() {
        if (!visible || !scanning || scanner != null || !preview.getHolder().getSurface().isValid()) return;
        scanner = new QrScanner(this, preview.getHolder().getSurface(), new QrScanner.Listener() {
            public void found(String text) { runOnUiThread(() -> { if (visible && scanning) pair(text); }); }
            public void error(String text) { runOnUiThread(() -> { if (visible && scanning) status.setText(text); }); }
            public void info(int orientation) { runOnUiThread(() -> { if (scanning) { sensor = orientation; updateAspect(); } }); }
        });
    }
    private void stopScanner() { if (scanner != null) { scanner.stop(); scanner = null; } }
    private int displayDegrees() { return getWindowManager().getDefaultDisplay().getRotation() * 90; }
    private void attachPreview() {
        if (!scanning && visible && service != null) {
            Surface surface = preview.getVisibility() == View.VISIBLE && preview.getHolder().getSurface().isValid()
                ? preview.getHolder().getSurface() : null;
            service.preview(surface, displayDegrees());
        }
    }
    private void updateAspect() { preview.cameraInfo(captureWidth, captureHeight, sensor, front, displayDegrees()); }
    private void showPreview(boolean enabled) {
        preview.setVisibility(enabled ? View.VISIBLE : View.INVISIBLE);
        placeholder.setVisibility(enabled ? View.GONE : View.VISIBLE);
    }
    @Override public void state(String text, boolean phonePreview, boolean front, boolean streaming) {
        status.setText(text); this.streaming = streaming; this.front = front; this.phonePreview = phonePreview;
        boolean sharing = CameraService.available;
        if (!scanning) showPreview(phonePreview && streaming && sharing);
        if (!sharing && bound) detachControl(); else attachPreview();
        refresh();
    }
    @Override public void cameraInfo(int width, int height, int sensor, boolean front) {
        captureWidth = width; captureHeight = height; this.sensor = sensor; this.front = front;
        updateAspect(); preview.getHolder().setFixedSize(width, height);
    }
    @Override public void onDisplayChanged(int id) { updateAspect(); if (service != null) service.rotation(displayDegrees()); }
    @Override public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(code, permissions, results);
        if (!cameraAllowed()) {
            pendingEnable = false; pendingScan = false;
            status.setText("Camera access is required. Enable it in Android app settings."); return;
        }
        if (code == 4 || code == 5) main.postDelayed(() -> {
            if (!visible) return;
            if (pendingEnable) {
                String pairing = pendingPairing; pendingEnable = false; enableControl(pairing);
            } else if (pendingScan) { pendingScan = false; beginScan(); }
        }, 100);
    }
    @Override public void onDisplayAdded(int id) { }
    @Override public void onDisplayRemoved(int id) { }
    @Override public void onConfigurationChanged(Configuration config) { super.onConfigurationChanged(config); onDisplayChanged(0); }
}
