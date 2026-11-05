package app.camerabridge;

import android.Manifest;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.content.pm.ServiceInfo;
import android.net.wifi.WifiManager;
import android.os.*;
import android.view.Surface;
import org.json.JSONObject;
import java.io.*;
import java.lang.ref.WeakReference;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.cert.X509Certificate;
import java.util.Locale;
import java.util.concurrent.atomic.AtomicReference;
import javax.net.ssl.*;

/** User-enabled camera service. Survives backgrounding, but stops when its task is removed. */
public final class CameraService extends Service {
    static final String ARM = "app.camerabridge.ARM", STOP = "app.camerabridge.STOP";
    private static final int NOTIFICATION = 41;
    private static final String CHANNEL = "camerabridge-camera";
    static volatile boolean available;
    interface Listener {
        void state(String text, boolean preview, boolean front, boolean streaming);
        void cameraInfo(int width, int height, int sensor, boolean front);
    }
    public final class LocalBinder extends Binder { CameraService service() { return CameraService.this; } }
    private final LocalBinder binder = new LocalBinder();
    private final Handler main = new Handler(Looper.getMainLooper());
    private WeakReference<Listener> listener = new WeakReference<>(null);
    private final AtomicReference<JSONObject> pendingStatus = new AtomicReference<>();
    private CameraEngine engine;
    private volatile StreamOptions options = StreamOptions.defaults();
    private volatile boolean armed, connected;
    private volatile String pairingText, message = "Ready for PC control", error = "";
    private volatile SSLSocket socket;
    private Thread network;
    private PowerManager.WakeLock wake;
    private WifiManager.WifiLock wifi;
    private int captureWidth = 1280, captureHeight = 720, sensor = 90;

    @Override public void onCreate() {
        super.onCreate();
        NotificationManager notifications = (NotificationManager)getSystemService(NOTIFICATION_SERVICE);
        notifications.createNotificationChannel(new NotificationChannel(CHANNEL, "Phone webcam", NotificationManager.IMPORTANCE_LOW));
        engine = new CameraEngine(this, new CameraEngine.Listener() {
            public void info(int width, int height, int orientation, boolean front) {
                captureWidth = width; captureHeight = height; sensor = orientation;
                main.post(() -> {
                    Listener target = listener.get();
                    if (target != null) target.cameraInfo(width, height, orientation, front);
                });
            }
            public void state(String text, boolean failed) {
                error = failed ? text : "";
                publish(text);
            }
        });
    }

    @Override public int onStartCommand(Intent intent, int flags, int id) {
        if (intent != null && STOP.equals(intent.getAction())) { stopSharing(); return START_NOT_STICKY; }
        try {
            if (checkSelfPermission(Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED)
                throw new SecurityException("Open CameraBridge and allow camera access");
            if (Build.VERSION.SDK_INT >= 29) startForeground(NOTIFICATION, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA);
            else startForeground(NOTIFICATION, notification());
            String value = intent == null ? null : intent.getStringExtra("pairing");
            if (value == null) value = getSharedPreferences("camera", MODE_PRIVATE).getString("pairing", null);
            if (value == null) throw new IllegalArgumentException("Pair with a PC first");
            validatePairing(value);
            pairingText = value;
            getSharedPreferences("camera", MODE_PRIVATE).edit().putString("pairing", value).apply();
            available = true; armed = true;
            acquirePower();
            if (network == null || !network.isAlive()) {
                network = new Thread(this::networkLoop, "CameraBridge Wi-Fi"); network.start();
            } else disconnectSocket();
        } catch (Exception failure) {
            publish(failure.getMessage()); stopSelf();
        }
        // A force stop or OS kill must be re-enabled visibly; do not attempt an illegal background FGS restart.
        return START_NOT_STICKY;
    }

    @Override public IBinder onBind(Intent intent) { return binder; }

    @Override public void onTaskRemoved(Intent rootIntent) {
        // Swiping the app out of Recents is an explicit stop, unlike Home or screen lock.
        // Clear wanted as well so reopening the app cannot silently resume PC access.
        stopSharing();
        super.onTaskRemoved(rootIntent);
    }

    void listen(Listener value) {
        listener = new WeakReference<>(value);
        if (value != null) {
            value.cameraInfo(captureWidth, captureHeight, sensor, options.front);
            value.state(message, options.preview, options.front, connected && options.enabled);
        }
    }
    void preview(Surface surface, int degrees) { engine.preview(surface, degrees); }
    void rotation(int degrees) { engine.rotation(degrees); }
    boolean previewEnabled() { return options.preview; }
    boolean connected() { return connected; }
    void setPhonePreview(boolean enabled) {
        options = options.withPreview(enabled);
        engine.apply(options, connected);
        publish(enabled ? "Phone preview enabled" : "Phone preview off; webcam remains available");
    }
    void switchLens() {
        options = options.withFront(!options.front);
        engine.apply(options, connected); publish("Camera switched");
    }
    void setStreaming(boolean enabled) {
        options = options.withEnabled(enabled);
        engine.apply(options, connected); publish(enabled ? "Starting camera" : "Ready for PC control; camera stopped");
    }

    static JSONObject validatePairing(String text) throws Exception {
        JSONObject value = new JSONObject(text.trim());
        if (value.getInt("v") != 1 || !value.getString("token").matches("[0-9a-f]{64}")
            || !value.getString("fingerprint").matches("[0-9a-f]{64}")) throw new Exception("Invalid PC pairing code");
        String host = value.getString("host");
        if (!host.matches("[0-9]{1,3}(\\.[0-9]{1,3}){3}")) throw new Exception("Invalid PC address");
        for (String part : host.split("\\.")) if (Integer.parseInt(part) > 255) throw new Exception("Invalid PC address");
        int port = value.getInt("port");
        if (port < 1 || port > 65535) throw new Exception("Invalid port");
        return value;
    }

    private JSONObject snapshot() {
        try {
            StreamOptions current = options;
            return new JSONObject().put("revision", current.revision).put("phone_preview", current.preview)
                .put("lens", current.front ? "front" : "rear").put("streaming", connected && current.enabled)
                .put("active", engine.active).put("message", message).put("error", error);
        } catch (Exception impossible) { throw new IllegalStateException(impossible); }
    }

    private void publish(String text) {
        message = text == null ? "CameraBridge ready" : text;
        pendingStatus.set(snapshot());
        main.post(() -> {
            Listener target = listener.get();
            if (target != null) target.state(message, options.preview, options.front, connected && options.enabled);
        });
    }

    private Notification notification() {
        Intent open = new Intent(this, MainActivity.class);
        PendingIntent content = PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        PendingIntent stop = PendingIntent.getService(this, 1, new Intent(this, CameraService.class).setAction(STOP),
            PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        return new Notification.Builder(this, CHANNEL).setSmallIcon(R.drawable.ic_notification)
            .setContentTitle("CameraBridge · PC control enabled")
            .setContentText("Camera can be controlled by your paired PC, including while locked.")
            .setContentIntent(content).setOngoing(true).setOnlyAlertOnce(true)
            .addAction(new Notification.Action.Builder(null, "Stop sharing", stop).build()).build();
    }

    private void acquirePower() {
        if (wake == null) {
            PowerManager power = (PowerManager)getSystemService(POWER_SERVICE);
            wake = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "CameraBridge:armed");
            wake.setReferenceCounted(false);
        }
        if (!wake.isHeld()) wake.acquire();
        WifiManager manager = (WifiManager)getApplicationContext().getSystemService(WIFI_SERVICE);
        if (wifi == null && manager != null) {
            wifi = manager.createWifiLock(WifiManager.WIFI_MODE_FULL, "CameraBridge:local-control");
            wifi.setReferenceCounted(false);
        }
        if (wifi != null && !wifi.isHeld()) wifi.acquire();
    }

    private SSLSocket connect(JSONObject pairing) throws Exception {
        final String pin = pairing.getString("fingerprint");
        X509TrustManager trust = new X509TrustManager() {
            public X509Certificate[] getAcceptedIssuers() { return new X509Certificate[0]; }
            public void checkClientTrusted(X509Certificate[] chain, String type) throws java.security.cert.CertificateException {
                throw new java.security.cert.CertificateException("Not supported");
            }
            public void checkServerTrusted(X509Certificate[] chain, String type) throws java.security.cert.CertificateException {
                try {
                    if (chain.length == 0) throw new Exception("Missing PC certificate");
                    StringBuilder actual = new StringBuilder();
                    for (byte value : MessageDigest.getInstance("SHA-256").digest(chain[0].getEncoded()))
                        actual.append(String.format(Locale.ROOT, "%02x", value & 255));
                    if (!MessageDigest.isEqual(actual.toString().getBytes(StandardCharsets.US_ASCII), pin.getBytes(StandardCharsets.US_ASCII)))
                        throw new Exception("PC identity changed. Scan the new pairing code.");
                } catch (Exception error) { throw new java.security.cert.CertificateException(error); }
            }
        };
        SSLContext tls = SSLContext.getInstance("TLS"); tls.init(null, new TrustManager[]{trust}, null);
        SSLSocket connection = (SSLSocket)tls.getSocketFactory().createSocket();
        socket = connection;
        try {
            connection.setSoTimeout(5000); connection.setTcpNoDelay(true);
            connection.connect(new InetSocketAddress(pairing.getString("host"), pairing.getInt("port")), 5000);
            connection.startHandshake(); return connection;
        } catch (Exception error) { connection.close(); throw error; }
    }

    private static JSONObject readJson(InputStream input) throws Exception {
        ByteArrayOutputStream line = new ByteArrayOutputStream();
        int value;
        while ((value = input.read()) != '\n') {
            if (value < 0 || line.size() >= 4096) throw new IOException("Invalid PC response");
            line.write(value);
        }
        return new JSONObject(new String(line.toByteArray(), StandardCharsets.UTF_8));
    }

    // Rust owns reconnect/backoff and packet streaming. Android owns TLS sockets,
    // the foreground lifecycle, and Camera2/MediaCodec platform callbacks.
    private DataOutputStream output;
    private long statsStart, heartbeat;
    private int sent;

    private void networkLoop() { NativeBridge.run(this); }
    private boolean nativeArmed() { return armed; }
    private void nativeConnect() throws Exception {
        publish("Connecting to paired PC");
        JSONObject pairing = validatePairing(pairingText);
        SSLSocket active = connect(pairing);
        if (!armed) { active.close(); return; }
        output = new DataOutputStream(new BufferedOutputStream(active.getOutputStream(), 65536));
        JSONObject hello = new JSONObject().put("v", 2).put("codec", "h264")
            .put("stream_settings", true).put("remote_control", true).put("token", pairing.getString("token"));
        output.write((hello.toString() + "\n").getBytes(StandardCharsets.UTF_8)); output.flush();
        JSONObject reply = readJson(active.getInputStream());
        if (!reply.optBoolean("ok")) throw new IOException("Pairing rejected. Scan the PC code again.");
        if (!reply.optBoolean("remote_control")) throw new IOException("Update the PC app for remote control");
        options = new StreamOptions(reply.optInt("width",1280), reply.optInt("height",720), reply.optInt("fps",30),
            reply.optInt("bitrate",3000000), reply.optBoolean("steady_fps",true), "front".equals(reply.optString("lens","rear")),
            reply.optBoolean("phone_preview",true), reply.optBoolean("streaming",true), reply.optInt("revision",0));
        connected = true; error = "";
        engine.apply(options, true);
        publish(options.enabled ? "Starting camera; screen may be locked" : "Ready for PC control; camera stopped");
        statsStart = SystemClock.elapsedRealtime(); heartbeat = 0; sent = 0;
        engine.takeEncoded();
    }
    private byte[] nativeStatus() {
        JSONObject status = pendingStatus.getAndSet(null);
        return status == null ? null : status.toString().getBytes(StandardCharsets.UTF_8);
    }
    private CameraEngine.Packet nativePoll() throws InterruptedException { return engine.poll(options.enabled ? 200 : 1000); }
    private void nativeWrite(byte[] packet) throws IOException {
        output.write(packet); output.flush();
        if (packet.length > 8 && packet[4] != (byte)0xff) sent++;
    }
    private void nativeTick() throws IOException {
        long now = SystemClock.elapsedRealtime();
        if (now - heartbeat >= 1000) { output.writeLong(0); output.flush(); heartbeat = now; }
        if (now - statsStart >= 1000 && options.enabled) {
            double seconds = (now - statsStart)/1000.0;
            int encoded = engine.takeEncoded();
            // A failed encoder must remain visible instead of being overwritten by LIVE 0.
            if (!error.isEmpty()) publish(error);
            else if (engine.active) publish(String.format(Locale.ROOT, "LIVE · encoded %.0f / sent %.0f fps · Rust streaming",
                encoded/seconds, sent/seconds));
            sent = 0; statsStart = now;
        }
    }
    private void nativeFailure(String failure) { if (armed) publish("Waiting for PC · " + failure); }
    private void nativeDisconnect() {
        connected = false; disconnectSocket(); socket = null; output = null;
        engine.apply(options, false);
    }

    private void disconnectSocket() {
        SSLSocket active = socket;
        if (active != null) try { active.close(); } catch (IOException ignored) { }
    }
    void stopSharing() {
        armed = false; available = false; connected = false;
        getSharedPreferences("camera", MODE_PRIVATE).edit().putBoolean("wanted", false).apply();
        disconnectSocket(); engine.apply(options, false);
        if (network != null) network.interrupt();
        publish("Sharing stopped. PC camera access is disabled.");
        stopForeground(STOP_FOREGROUND_REMOVE);
        if (wifi != null && wifi.isHeld()) wifi.release();
        if (wake != null && wake.isHeld()) wake.release();
        stopSelf();
    }

    @Override public void onDestroy() {
        armed = false; available = false; connected = false;
        disconnectSocket();
        if (network != null) network.interrupt();
        engine.shutdown();
        if (wifi != null && wifi.isHeld()) wifi.release();
        if (wake != null && wake.isHeld()) wake.release();
        listener.clear(); stopForeground(STOP_FOREGROUND_REMOVE);
        super.onDestroy();
    }
}
