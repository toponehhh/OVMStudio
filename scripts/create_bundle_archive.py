"""Create and verify a UI-only application archive using the Python stdlib."""
import hashlib
import json
import sys
from pathlib import Path
import zipfile

source = Path(sys.argv[1]).resolve()
target = Path(sys.argv[2]).resolve()
temporary = target.with_suffix('.zip.tmp')
print('Creating portable archive...', flush=True)
with zipfile.ZipFile(temporary, 'w', zipfile.ZIP_DEFLATED, compresslevel=5) as archive:
    for path in sorted(source.rglob('*')):
        if path.is_file():
            archive.write(path, Path(source.name) / path.relative_to(source))
print('Verifying archive contents and CRC...', flush=True)
with zipfile.ZipFile(temporary) as archive:
    required = {'OVMS-Studio/OVMS-Studio.exe', 'OVMS-Studio/LICENSE', 'OVMS-Studio/README.md',
                'OVMS-Studio/examples/tiny-add/1/model.onnx', 'OVMS-Studio/bundle-manifest.json'}
    missing = required.difference(archive.namelist())
    if missing:
        raise RuntimeError(f'Incomplete application archive: {sorted(missing)}')
    manifest = json.loads(archive.read('OVMS-Studio/bundle-manifest.json').decode('utf-8-sig'))
    if manifest.get('distribution') != 'ui-only' or manifest.get('runtime_delivery') != 'on-demand':
        raise RuntimeError('The release manifest must declare on-demand runtimes')
    if any(name.lower().startswith('ovms-studio/ovms/') for name in archive.namelist()):
        raise RuntimeError('An OVMS runtime must not be included in the UI archive')
    bad = archive.testzip()
    if bad:
        raise RuntimeError(f'Archive CRC error: {bad}')
    count = len(archive.namelist())
temporary.replace(target)
digest = hashlib.sha256(target.read_bytes()).hexdigest()
target.with_suffix('.zip.sha256').write_text(f'{digest}  {target.name}\n', encoding='ascii')
print(f'Created {target} ({target.stat().st_size:,} bytes, {count:,} files)', flush=True)
