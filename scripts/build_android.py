"""Build Rust streaming and Android platform adapters without Android Studio.

Downloads are cached inside .tools. Requires JDK 17+ on PATH. The local signing
key is generated once and kept outside source control. Never delete it if you
want to install upgrades over an existing APK.
"""
from pathlib import Path
import hashlib
import json
import os
import shutil
import subprocess
import urllib.request
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TOOLS = ROOT / ".tools"
SDK = TOOLS / "android-sdk"
BUILD = ROOT / "build" / "android"
DIST = ROOT / "dist"


def download(url, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        print("Downloading", url, flush=True)
        temporary = path.with_suffix(path.suffix + ".part")
        urllib.request.urlretrieve(url, temporary)
        temporary.replace(path)


def sdk_package(name, destination):
    marker = destination / ".ready"
    if marker.exists():
        return
    metadata = TOOLS / "repository2-3.xml"
    download("https://dl.google.com/android/repository/repository2-3.xml", metadata)
    document = ET.parse(metadata).getroot()
    package = next(x for x in document if x.tag.endswith("remotePackage") and x.attrib.get("path") == name)
    for archive in package.findall("./archives/archive"):
        host = archive.findtext("host-os")
        if host is None or host == "windows":
            complete = archive.find("complete")
            break
    else:
        raise RuntimeError("No Windows archive for " + name)
    filename = complete.findtext("url")
    archive_path = TOOLS / filename
    download("https://dl.google.com/android/repository/" + filename, archive_path)
    expected = complete.findtext("checksum")
    if hashlib.sha1(archive_path.read_bytes()).hexdigest() != expected:
        raise RuntimeError("SDK archive checksum mismatch: " + filename)
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive_path) as archive:
        top = archive.namelist()[0].split("/")[0] + "/"
        for info in archive.infolist():
            relative = info.filename.removeprefix(top)
            if relative and not info.is_dir():
                target = (destination / relative).resolve()
                if not target.is_relative_to(destination.resolve()):
                    raise RuntimeError("Invalid archive path")
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(archive.read(info))
    marker.touch()


def run(args):
    subprocess.run([str(x) for x in args], check=True, cwd=ROOT)


def main():
    sdk_package("platforms;android-35", SDK / "platforms" / "android-35")
    sdk_package("build-tools;35.0.0", SDK / "build-tools" / "35.0.0")
    ndk = SDK / "ndk" / "27.3.13750724"
    sdk_package("ndk;27.3.13750724", ndk)
    toolchain = ndk / "toolchains" / "llvm" / "prebuilt" / "windows-x86_64" / "bin"
    rust_env = os.environ.copy()
    rust_env["CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER"] = str(toolchain / "aarch64-linux-android26-clang.cmd")
    rust_env["CARGO_TARGET_AARCH64_LINUX_ANDROID_RUSTFLAGS"] = "-C link-arg=-Wl,-z,max-page-size=16384"
    subprocess.run(["rustup", "target", "add", "aarch64-linux-android"], check=True)
    subprocess.run(["cargo", "build", "--locked", "--release", "-p", "camerabridge-android", "--target", "aarch64-linux-android"],
                   cwd=ROOT, env=rust_env, check=True)
    zxing = TOOLS / "zxing-core-3.5.3.jar"
    download("https://repo1.maven.org/maven2/com/google/zxing/core/3.5.3/core-3.5.3.jar", zxing)
    # Verify dependency against the Maven Central checksum.
    checksum = zxing.with_suffix(".jar.sha1")
    download("https://repo1.maven.org/maven2/com/google/zxing/core/3.5.3/core-3.5.3.jar.sha1", checksum)
    if hashlib.sha1(zxing.read_bytes()).hexdigest() != checksum.read_text().strip():
        raise RuntimeError("ZXing checksum mismatch")
    android = SDK / "platforms" / "android-35" / "android.jar"
    bt = SDK / "build-tools" / "35.0.0"
    BUILD.mkdir(parents=True, exist_ok=True)
    DIST.mkdir(exist_ok=True)
    classes = BUILD / "classes"
    # Clean only these verified build outputs so removed Activity inner classes
    # cannot leak into a new APK after a lifecycle refactor.
    for output in (classes, BUILD / "dex"):
        resolved = output.resolve()
        if resolved.parent != BUILD.resolve() or resolved.name not in ("classes", "dex"):
            raise RuntimeError("Build cleanup escaped the Android build directory")
        if resolved.exists():
            shutil.rmtree(resolved)
    classes.mkdir(exist_ok=True)
    resources = BUILD / "resources.zip"
    run([bt / "aapt2.exe", "compile", "--dir",
         ROOT / "android/app/src/main/res", "-o", resources])
    generated = BUILD / "generated"
    generated.mkdir(exist_ok=True)
    unsigned = BUILD / "unsigned.apk"
    run([bt / "aapt2.exe", "link", "-I", android, "--manifest",
         ROOT / "android/app/src/main/AndroidManifest.xml", "--java", generated,
         "-o", unsigned, resources])
    sources = list((ROOT / "android" / "app" / "src" / "main" / "java").rglob("*.java"))
    sources += list(generated.rglob("*.java"))
    run(["javac", "-encoding", "UTF-8", "--release", "8", "-classpath",
         str(android) + os.pathsep + str(zxing), "-d", classes, *sources])
    jar = BUILD / "app.jar"
    run(["jar", "cf", jar, "-C", classes, "."])
    dex = BUILD / "dex"
    dex.mkdir(exist_ok=True)
    run(["java", "-cp", bt / "lib" / "d8.jar", "com.android.tools.r8.D8", "--release",
         "--lib", android, "--min-api", "26", "--output", dex, jar, zxing])
    with zipfile.ZipFile(unsigned, "a", zipfile.ZIP_DEFLATED, strict_timestamps=False) as archive:
        archive.write(ROOT / "target" / "aarch64-linux-android" / "release" / "libcamerabridge_android.so",
                      "lib/arm64-v8a/libcamerabridge_android.so")
        for file in dex.glob("*.dex"):
            archive.write(file, file.name)
        for file in (ROOT / "third_party" / "licenses").glob("ZXing-*.txt"):
            archive.write(file, "assets/licenses/" + file.name)
        for file in (ROOT / "third_party" / "licenses" / "rust").rglob("*"):
            if file.is_file():
                archive.write(file, "assets/licenses/rust/" + file.relative_to(ROOT / "third_party" / "licenses" / "rust").as_posix())
    aligned = BUILD / "aligned.apk"
    run([bt / "zipalign.exe", "-f", "4", unsigned, aligned])
    key = TOOLS / "camerabridge-local.jks"
    if not key.exists():
        run(["keytool", "-genkeypair", "-keystore", key, "-storepass", "camerabridge-local",
             "-keypass", "camerabridge-local", "-alias", "camerabridge", "-keyalg", "RSA",
             "-keysize", "3072", "-validity", "10000", "-dname", "CN=CameraBridge local development"])
    apk = DIST / "CameraBridge-Android.apk"
    run(["java", "-jar", bt / "lib" / "apksigner.jar", "sign", "--ks", key,
         "--ks-pass", "pass:camerabridge-local", "--out", apk, aligned])
    run(["java", "-jar", bt / "lib" / "apksigner.jar", "verify", "--verbose", apk])
    print("APK:", apk, flush=True)


if __name__ == "__main__":
    main()
