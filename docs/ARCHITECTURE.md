# CameraBridge 0.4.0 — Rust

Android-to-Windows Wi-Fi webcam, capped at 720p. The Windows receiver is a native
Rust executable using Win32 controls and GDI. It ships without Python, Tk,
Electron, a browser runtime, or a separate FFmpeg installation.

The Android streaming core is Rust: bounded frame queues, H.264 configuration /
keyframe handling, congestion recovery, rotation, FPS policy, packet framing,
and connection orchestration. Android UI, Camera2/MediaCodec callbacks, pinned
platform TLS, permissions, QR scanning and foreground-service lifecycle remain
Java platform adapters. This is not a Java-free Android rewrite. Camera frames
still go directly from Camera2 to a hardware encoder surface.

## Run

1. Close the old PC receiver. Open `dist/CameraBridge-Rust/CameraBridge.exe`.
   Keep its `third_party` folder beside it.
2. Use **Set up Windows webcam** if the webcam is not already installed, or to
   update the firewall rule for the new executable. Setup requires Windows
   elevation. Regular use does not. Restart Discord after first-time setup.
3. Install `dist/CameraBridge-Android.apk` over the old APK (ARM64, Android 8+).
4. Put both devices on the same private LAN. Scan the PC pairing QR code using
   the phone app. Existing pairing and certificates are reused from
   `%LOCALAPPDATA%/CameraBridge/identity`.
5. Select **CameraBridge** in Discord. Select your PC microphone separately.

The window uses a dark theme with a dark title bar, a left control column and a
large video area. A status pill and **Start camera / Stop camera** sit above the
video. **Pair phone** contains the QR code, address and **Copy pairing code**
button; **Settings** contains resolution, lens, frame rate, bitrate and preview controls.
It opens settings automatically when the first camera frames arrive.
If the PC selected the wrong adapter address, enter its LAN IPv4 address and
click **Update** before scanning. Treat the pairing code as a credential.

Resolution choices are **640x360, 960x540, 1280x720**. Saved 1080p preferences
migrate to 720p. Android rejects requests above 720p and will never select a
camera encoder surface above that limit. FPS choices are 15, 20, 24 and 30;
bitrate is 1–8 Mb/s. **Apply changes** preserves the current start/stop state.
The main **Start camera / Stop camera** button controls capture; Stop leaves PC control armed.
Preview FPS **Off** skips PC preview conversion/drawing; minimizing also skips
preview work. Phone preview, front/rear lens, steady FPS and horizontal mirror
are independent controls.

## Upright webcam output

The receiver keeps preview pixels top-down. At the Unity Capture shared-memory
boundary it writes RGBA rows bottom-up, matching the DirectShow filter's positive
bitmap height. This fixes the vertically inverted output without altering the
phone rotation or the optional horizontal mirror. The existing camera DLLs do
not need replacement for this correction.

## Resource use and differences from 0.3

The Rust receiver retains only the latest decoded frame and reuses conversion
buffers. Android's queue holds at most four encoded frames and waits for a fresh
keyframe after congestion. TLS pinning, persistent pairing and foreground camera
control remain in place. No video is recorded.

The current Rust Windows decoder uses **OpenH264 software decoding**. The old
Python receiver's D3D11 decoder selection is not yet ported. A smaller executable
does not prove lower streaming CPU use; test your actual phone, Wi-Fi and Discord
before drawing performance conclusions. Rotation and letterboxing use bilinear
sampling; the PC preview uses high-quality Windows scaling, DPI-aware fonts
and a reusable back buffer to avoid flicker. Preview timing uses absolute
deadlines rather than accumulating polling delays. The native filter's maximum shared mapping capacity remains about
64 MiB, with only active image pages touched by this receiver.

The Windows UI has direct controls instead of the old presets/settings dialog
and preview context menu. The legacy Python sources and their build history
remain for reference; see [legacy documentation](LEGACY_README.md).

## Build and verify

Requirements: Rust stable (1.99+), Visual Studio C++ build tools, JDK 17+, Python
3.12+ for APK packaging only. The Android script caches Google's SDK/NDK tools
under `.tools`. The original `.tools/camerabridge-local.jks` must be retained to
install compatible APK updates. Android APK output is ARM64 only.

```powershell
cargo test --locked --workspace
cargo clippy --workspace --all-targets -- -D warnings
powershell -File scripts/check_jni.ps1
powershell -File scripts/build_windows.ps1
python scripts/build_android.py
python scripts/package_release.py
```

`CameraBridge.exe --self-test` runs an isolated loopback TLS / H.264 / webcam
mapping test, verifies row order, settings and pairing persistence, and prints
JSON. It uses its own temporary identity and uniquely named webcam mapping;
it does not publish test video into the installed webcam. Packaging requires
this self-test to pass. JNI tests run real native Rust through the JVM with
`-Xcheck:jni`, including reconnect exceptions and service teardown.

Phone camera behavior, prolonged screen-lock capture and Discord consumption
still require testing on the real devices. Keep Android battery restrictions
set to unrestricted for CameraBridge if the phone suspends local networking.
A foreground-service notification exposes Stop sharing; after reboot or force
stop, reopen the app to enable PC control again.
