"""Fetch the pinned official OVMS fixture for tests; it is never packaged with the UI."""
import hashlib
import json
from pathlib import Path
import urllib.request
import zipfile

root = Path(__file__).resolve().parent.parent / 'work'
root.mkdir(exist_ok=True)
name = 'ovms_windows_2026.4.1_python_on.zip'
url = 'https://github.com/openvinotoolkit/model_server/releases/download/v2026.4.1/' + name
# Published at the same release URL with the .sha256 suffix; verified on 2026-11-02.
expected = 'aa79a0ae3b858356ba550978e397a8e8876a9d2706ffe4041b1dfd3f32cb8ce4'
archive = root / name
if not archive.exists():
    print('Downloading official OVMS 2026.4.1 with Python support...', flush=True)
    urllib.request.urlretrieve(url, archive)
actual = hashlib.sha256(archive.read_bytes()).hexdigest()
if actual != expected:
    raise RuntimeError('OVMS archive SHA-256 mismatch')
destination = root / 'runtime-python-on'
destination.mkdir(exist_ok=True)
with zipfile.ZipFile(archive) as source:
    source.extractall(destination)
metadata = {'version': '2026.4.1', 'flavor': 'python_on', 'source_url': url, 'archive_sha256': actual}
(destination / 'runtime-source.json').write_text(json.dumps(metadata, indent=2) + '\n', encoding='utf-8')
print('Official runtime verified and extracted:', destination, flush=True)
