# CameraBridge 0.3.0

A standalone Android-to-Windows Wi-Fi webcam. Built for a Redmi Note 12 Pro
on Android 14 and a Windows 10 x64 PC. No OBS, cloud account, browser, or
Python installation is required to use the packaged apps.

## Update from an earlier version

Close the old Windows receiver, replace its entire folder with the new Windows
package, and install the new Android APK over the existing app. Open both and
scan the current pairing code. The native camera DLLs are unchanged, so an
existing webcam registration can be reused. If the receiver moved to a different
folder, rerun webcam setup to update its firewall executable path.

The old PC preview was deliberately capped at 5 fps. The new preview defaults
to the stream's 30-fps target, with an independent drawing timer and a live FPS
counter. Install **both** updated apps to use background capture and PC camera
controls. Pair once after upgrading; pairing now survives PC app restarts.

## First use

1. Open `dist/CameraBridge/CameraBridge.exe`. Keep the entire folder together.
2. Click **Set up Windows webcam** and approve Windows elevation. This
   registers the bundled 32-bit and 64-bit DirectShow filters as **CameraBridge**
   and allows the receiver through Windows Firewall on **Private** networks,
   from the local subnet only. Restart any video apps that were already open.
3. Transfer `dist/CameraBridge-Android.apk` to the phone and open it. Android
   will require allowing this source to install the locally built APK.
4. Put the phone and PC on the same private/home LAN. The PC can use Ethernet.
5. Open CameraBridge on the phone and grant camera/notification permission.
   Tap **Scan PC pairing code** and scan the Windows code. Pairing automatically
   enables foreground **PC control** and connects the webcam. Allow the system's
   battery exemption prompt for a persistent connection with the screen off.
   If necessary, select another PC address above the code.
   Pairing text can alternatively be copied on the PC and pasted on the phone.
6. Select **CameraBridge** in Discord, Zoom, Teams, OBS, or the browser's camera
   selector. Select your PC microphone separately.

## Settings

Click **Settings** at the bottom of the Windows receiver. Preferences are stored
in `%LOCALAPPDATA%\CameraBridge\settings.json` and restored on the next launch.

- **Resolution:** 640×360, 960×540, 1280×720 or 1920×1080.
- **Stream FPS:** 15, 20, 24 or 30. This is a target, subject to the camera and network.
- **Bitrate:** 1–8 Mb/s. Lower values reduce network traffic; higher values retain more detail.
- **Preview FPS:** off, 5, 15 or 30. This controls only the PC preview, independently of webcam output.
- **Decoder:** Auto uses D3D11 when available; CPU lets you bypass problematic GPU drivers.
- **Steady FPS:** prefers a fixed/high-floor exposure range, which prevents the
  camera dropping to a very low rate in a dim room. It can make a dark room look darker.
- **Mirror:** flips the webcam output.
- **Phone camera:** selects front or rear camera from the PC.
- **Show preview on the phone:** removes the phone viewfinder when disabled;
  the camera still feeds the encoder and Windows webcam.

Balanced, Smooth on slower Wi-Fi, More detail, and Power saver presets are
available. **Apply settings automatically starts the phone camera**, including
when the phone is locked, provided PC control is already enabled there. Changes
to capture/decoder/phone-preview settings reconnect with the new configuration.
The phone foreground service performs that reconnect without any Activity tap.

The PC's **Start camera / Stop camera** button controls actual phone capture.
Stopping capture leaves the authenticated control connection available so the PC
can restart it. **Stop sharing** on the phone (or its notification) disarms the
service entirely and prevents further remote starts until it is enabled again.

Right-click the PC preview for **Disable PC preview**, **Enable PC preview**,
phone-preview controls, start/stop, and Settings. Disabling either preview leaves
webcam video running; the right-click/preview checkbox actions do not start a
camera that you deliberately stopped. Phone-side preview/lens/stop controls sync
back to the PC.

The PC shows the measured received FPS and preview FPS separately. The Android
app shows encoded and sent FPS while streaming, making it easier to tell a slow
camera from a slow PC preview. Per-second receive/decode metrics are logged in
`%LOCALAPPDATA%\CameraBridge\receiver.log`.

The PC app does not need administrator rights for everyday use. The setup step
does. Native camera DLLs stay in `C:\Program Files\CameraBridge\Camera`, so moving
the portable receiver will not break their registration. If the receiver moves,
rerun setup to update the firewall rule's executable path.

## Performance choices

- Camera2 sends camera frames directly to a **hardware H.264 encoder surface**.
  Streaming does not copy raw YUV frames into Java or compress JPEGs.
- Default target: **1280×720, 30 fps, 3 Mb/s**. The camera chooses the closest supported
  preview/encoder size; output is normalized to 1280×720 on Windows.
- Baseline H.264, no B frames, frequent keyframes; packet queues are bounded to
  four compressed access units. Congestion discards pending data and requests
  a new keyframe rather than building an unbounded delay.
- Windows attempts D3D11 hardware decode, with a two-thread software fallback.
- If your video app offers camera format controls, select **1280×720** at
  your selected stream frame rate. A consumer requesting 1080p
  causes the native bridge to upscale, which adds work without extra detail.
- Only the latest decoded image is retained by the receiver. The desktop preview
  uses a separate clock, defaults to a 30-fps cap, and updates only when a new
  frame arrives. Absolute deadlines avoid accumulating timer drift; brief retries
  avoid waiting a whole additional frame if capture and drawing are out of phase.
- Preview images resize directly from the source, avoiding a full-frame copy.
  Installation/status polling runs separately once per second.
- Unchanged camera frames reuse the existing shared buffer. No Electron,
  Chromium, Node runtime, NumPy, or background cloud service is included.
- Webcam publication wakes on new decoded frames rather than sampling them on
  a second timer; an idle heartbeat maintains the native camera while waiting.
- Uncheck **Preview** or minimize the receiver to skip live preview rendering
  while continuing to publish the webcam to your video apps.
- QR scanning reads low-resolution luma frames only while pairing. Stopping
  capture releases the camera and hardware encoder while keeping PC control alive.
- The phone uses a letterboxed **SurfaceView** whose orientation is handled by
  Android, rather than reapplying sensor rotation in a TextureView matrix.
  Both landscape directions are handled through display-change events. Rolling
  the phone at 45 degrees does not introduce an additional image transform.
- With the phone viewfinder disabled or the screen locked, camera sessions use
  only the encoder surface. The foreground service uses CPU/Wi-Fi locks to keep
  a local connection available; these are released when sharing is disabled.
  PC-unreachable retries back off instead of reconnecting rapidly forever.

Actual CPU, memory, latency, and frame rate depend on both devices and the
network. No measurements on the Redmi have been made yet. The native bridge
uses an upstream shared mapping with a roughly 64 MiB maximum capacity; working
set and committed memory are different from the actual video frame size.

## Privacy and connection behavior

Video uses TLS 1.2 or later. The QR code includes a random pairing token and the
SHA-256 fingerprint of the PC certificate; Android pins that exact certificate
before sending credentials or video. The token and certificate persist across
PC restarts, and the phone stores its paired PC in app-private preferences.
No footage is recorded, and no server account is used. Treat pairing text as a
trusted-device access credential. The PC identity is stored under
`%LOCALAPPDATA%\CameraBridge\identity`; diagnostic logs are in `receiver.log`.

The phone foreground service retries while PC control is enabled, including
when its Activity is hidden or the screen is off. Capture stops while the PC is
unreachable. Opening the PC receiver again reconnects using the existing pairing
and its current camera settings. Stopping or disconnecting replaces webcam output
with a standby image instead of retaining the last private camera frame.

To revoke the PC pairing, close the PC receiver, remove
`identity/pairing-token.txt`, restart the receiver, and pair the intended phone
again. The pinned certificate may remain. Stop sharing on the phone to end its
foreground service and release its power/network locks.

## Screen off and Redmi battery settings

Enable PC control while the phone app is visible once. A persistent foreground
notification provides **Stop sharing**. You can then press the power button to
lock/turn off the screen; a fully powered-off phone cannot capture video.

Use **Battery / screen-off settings** in the phone app to allow the Android
battery-optimization exemption. On Redmi/MIUI/HyperOS, also allow CameraBridge
background activity / **No restrictions** if the vendor battery manager suspends
it. The first connection offers the system exemption prompt. This is required
for reliable local networking in prolonged device idle; a foreground service
alone does not exempt networking from all Doze restrictions.

After a reboot, force stop, permission revocation, or OS process kill, reopen the
phone app once to enable its camera foreground service again. The app does not
attempt a prohibited camera-service cold start from the background. Screen lock
and ordinary Activity dismissal do not stop its service. Device-specific battery
behavior and sustained locked-screen capture still require a Redmi test.

## Limits and troubleshooting

- Video only; no phone microphone, Bluetooth transport or USB transport.
- Calls or another camera app may interrupt camera access. The foreground
  service retries transient camera errors; PC settings can restart capture.
- Windows 10 uses a DirectShow virtual camera. Desktop apps and browsers are
  intended targets. Universal compatibility, Windows Hello, protected capture,
  and Media-Foundation-only / UWP applications are **not guaranteed**.
- Physical Redmi capture, orientation, reconnection, and each target application's
  camera picker still require device testing. A successfully built APK does not
  establish compatibility with every camera firmware.
- Guest Wi-Fi/client isolation, VPN routing, and Public firewall profiles can
  block the connection. Use a trusted Private LAN; do not disable the firewall.
- Existing UnityCapture installations use the same upstream COM identities.
  Setup refuses to overwrite one. Remove it with its own uninstaller first.
- The development EXE and APK are not signed by a commercial publisher.

## Remove the webcam

Close camera apps. In an elevated PowerShell, run:

```powershell
& 'C:\Program Files\CameraBridge\Camera\Uninstall-Camera.ps1'
```

This unregisters the native camera and removes its firewall rule. The portable
receiver folder, APK, and per-user identity can then be removed separately.

## Build from source

Windows build prerequisites: Python 3.12 x64, JDK 17+ for Android, internet for
initial dependencies. Build tools and SDK archives stay in `.tools/`.

```powershell
python -m venv .venv
.\.venv\Scripts\python.exe -m pip install -r desktop\requirements-lock.txt
.\.venv\Scripts\python.exe -m unittest discover -s tests -v
.\.venv\Scripts\python.exe scripts\smoke_native.py
.\.venv\Scripts\python.exe scripts\check_preview.py
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build_windows.ps1
python scripts\build_android.py
```

The Android build uses official Android API 35/build tools and the ZXing core
JAR, without requiring Android Studio or Gradle. The build script verifies SDK
archives and ZXing against repository checksums. Keep
`.tools/camerabridge-local.jks` to sign upgrades with the same local key.

The native smoke check loads the filter directly into a DirectShow graph with
Windows' Null Renderer, runs it, and stops it; it does not register a camera.
`scripts/check_performance.py` runs a synthetic local H.264/TLS/frame-buffer
smoke benchmark, excluding the phone, Wi-Fi, GUI, and receiving video app.
`scripts/check_preview.py` checks the actual Tk rendering path against synthetic
720p/30-fps frames, including a low-FPS cap and disabling the preview. This catches
preview throttling that a decoder-only benchmark cannot detect.
The pure Java frame-rate, geometry and camera-control policies can be checked
without an Android device:

```powershell
javac -encoding UTF-8 -d build\java-tests android\app\src\main\java\app\camerabridge\StreamPolicy.java android\app\src\main\java\app\camerabridge\StreamOptions.java android\app\src\main\java\app\camerabridge\PreviewGeometry.java tests\StreamPolicyTest.java tests\PhoneControlTest.java
java -cp build\java-tests StreamPolicyTest
java -cp build\java-tests PhoneControlTest
```

`CameraBridge.exe --self-test <absolute-output-json-path>` checks the frozen
package in an isolated mode without opening a LAN listener or installing a camera.

## Wire protocol

TCP port 47950 inside pinned TLS. The phone sends a bounded newline-terminated
JSON handshake `{ "v": 2, "codec": "h264", "remote_control": true, "stream_settings": true, "token": "..." }`.
Version 1 video senders remain supported by the Windows receiver.
The PC responds with `ok`, output dimensions, target frame rate, bitrate in
bits/second, `steady_fps`, `phone_preview`, `lens`, `streaming`, and a settings
`revision`. The Android service reads those settings before
configuring its camera/encoder. Every video message contains two
big-endian uint32 values (H.264 access-unit length, clockwise rotation), followed
by Annex-B H.264 bytes. Access units are limited to 2 MiB. Keyframes include
SPS/PPS, so reconnection and congestion recovery start at a clean decoder state.
Negotiated v2 sessions also allow zero-length heartbeat frames and bounded JSON
status messages tagged with rotation `0xFFFFFFFF`. They keep an idle camera
controllable and sync phone-side changes. PC changes close the authenticated
session; the still-armed foreground service reconnects automatically to obtain
the new desired configuration. Revision checks reject acknowledgements from
superseded sessions. Camera control never depends on recreating a phone Activity.
