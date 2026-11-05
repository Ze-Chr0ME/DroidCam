"""Validate and package the Rust receiver and signed ARM64 Android APK."""
from pathlib import Path
import hashlib
import json
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    dist = ROOT / 'dist'
    package = dist / 'CameraBridge-Rust'
    exe = package / 'CameraBridge.exe'
    apk = dist / 'CameraBridge-Android.apk'
    checked = subprocess.run([str(exe), '--self-test'], capture_output=True, text=True, check=True)
    report = json.loads(checked.stdout)
    if not report.get('ok'):
        raise RuntimeError('Rust packaged self-test failed')
    report['device_testing'] = 'Phone capture, prolonged screen lock and Discord still require real-device verification.'
    report['android_architecture'] = 'Rust streaming core with Java Android platform adapters; ARM64 only.'
    report['decoder'] = 'OpenH264 software; D3D11 is not ported.'
    (package / 'VALIDATION.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    archive_path = dist / 'CameraBridge-Windows-Rust.zip'
    temporary = archive_path.with_suffix('.zip.part')
    with zipfile.ZipFile(temporary, 'w', zipfile.ZIP_DEFLATED, compresslevel=6, strict_timestamps=False) as archive:
        for path in sorted(package.rglob('*')):
            if path.is_file():
                archive.write(path, path.relative_to(dist))
    with zipfile.ZipFile(temporary) as archive:
        if archive.testzip() is not None:
            raise RuntimeError('Archive integrity check failed')
    temporary.replace(archive_path)
    paths = [archive_path, apk]
    (dist / 'SHA256SUMS-Rust.txt').write_text('\n'.join(hashlib.sha256(p.read_bytes()).hexdigest() + '  ' + p.name for p in paths) + '\n', encoding='ascii')
    print(json.dumps(report))
    for path in paths:
        print(path, path.stat().st_size, 'bytes')


if __name__ == '__main__':
    main()
