# Third-party components

CameraBridge's Windows webcam uses **Unity Capture** by Bernhard Schelling,
based on UnityCam by MHD Yamen Saraiji. The DirectShow filter is MIT licensed.
The original source notice is preserved in
`third_party/unitycapture/UnityCaptureFilter.cpp`. The DLLs are unmodified from:
https://github.com/schellingb/UnityCapture
commit `3ed54c325e0ad71afcf4f246c07e5e17b3d7f2d2`.
No Unity runtime is required. This is independent of OBS.

Rust 0.4 dependencies are pinned in `Cargo.lock`. Exact versions, SPDX license
identifiers, repositories and upstream license texts are collected in
`third_party/licenses/rust/` by `python scripts/rust_licenses.py`. The Windows
receiver links OpenH264 (BSD-2-Clause), Rustls, ring, rcgen, serde, the Rust
qrcode crate, windows-sys and their dependencies. The Android JNI core uses
the jni and serde crates. OpenH264's upstream license is included alongside
the Rust wrapper license. No Python runtime, PyAV or FFmpeg library ships in
the Rust Windows package.

Android also uses ZXing core 3.5.3 (Apache-2.0). SDK and NDK are build tools;
the APK bundles the compiled Rust library, not the SDK/NDK tools.

Legacy Python receiver dependencies (not part of the Rust package):

- ZXing core 3.5.3 (Apache-2.0): https://github.com/zxing/zxing
- PyAV 16.0.1 (BSD-3-Clause): https://github.com/PyAV-Org/PyAV
- FFmpeg libraries bundled in the PyAV wheel: https://ffmpeg.org/legal.html
  The wheel's bundled license information is retained in the Windows package.
- Pillow 11.3.0 (HPND): https://github.com/python-pillow/Pillow
- cryptography 46.0.3 (Apache-2.0 OR BSD-3-Clause): https://github.com/pyca/cryptography
- qrcode 8.2 (BSD): https://github.com/lincolnloop/python-qrcode
- Python / Tk: https://docs.python.org/3/license.html
- PyInstaller 6.16.0 (GPL with bootloader distribution exception): https://pyinstaller.org/en/stable/license.html

The local APK build uses a development signing key generated into `.tools/`.
It is not a store release key. Retain that file to sign compatible local upgrades.
Android SDK build tools are downloaded from Google's official repository;
they are build-time dependencies and are not shipped inside the APK.
