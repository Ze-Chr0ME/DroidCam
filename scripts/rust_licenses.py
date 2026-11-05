"""Collect exact locked Rust dependency metadata and upstream license texts."""
from pathlib import Path
import json
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]

def main():
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=ROOT))
    output = ROOT / 'third_party/licenses/rust'
    output.mkdir(parents=True, exist_ok=True)
    packages = []
    for package in metadata['packages']:
        if package['source'] is None:
            continue
        source = Path(package['manifest_path']).parent
        destination = output / (package['name'] + '-' + package['version'])
        destination.mkdir(exist_ok=True)
        licenses = [p for p in source.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE', 'COPYING', 'NOTICE'))]
        if package['name'] == 'openh264-sys2':
            licenses += list((source / 'upstream').glob('LICENSE*'))
        for path in licenses:
            shutil.copy2(path, destination / path.name)
        packages.append(dict(name=package['name'], version=package['version'], license=package['license'],
                             repository=package['repository'], texts=[p.name for p in licenses]))
    (output / 'dependencies.json').write_text(json.dumps(packages, indent=2) + '\n', encoding='utf-8')
    print('Collected notices for', len(packages), 'locked Rust dependencies')

if __name__ == '__main__':
    main()
