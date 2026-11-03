"""Verify the UI-only package, fresh managed download and offline cache reuse."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import urllib.request
import uuid

project = Path(__file__).resolve().parent.parent
bundle = Path(sys.argv[1]).resolve()
assert not (bundle / 'ovms').exists(), 'OVMS must not be included in the UI package'
manifest = json.loads((bundle / 'bundle-manifest.json').read_text(encoding='utf-8-sig'))
assert manifest['distribution'] == 'ui-only' and manifest['runtime_delivery'] == 'on-demand'
output = project / 'work' / ('bundle-check-' + uuid.uuid4().hex)
output.mkdir(parents=True)
unrelated = output / 'unrelated working directory'
unrelated.mkdir()
data = output / 'managed runtime cache'
windows = Path(os.environ['SystemRoot'])
env = {k: v for k, v in os.environ.items() if k.upper() not in
       {'PATH', 'PYTHONHOME', 'PYTHONPATH', 'OVMS_HOME', 'OVMS_MODEL_REPOSITORY_PATH', 'OVMSTUDIO_DATA_DIR'}}
env['PATH'] = str(windows / 'System32') + os.pathsep + str(windows)
env['OVMSTUDIO_DATA_DIR'] = str(data)
env['OVMS_HOME'] = str(output / 'nonexistent global runtime')
env['PYTHONHOME'] = str(output / 'nonexistent global Python')
env['PYTHONPATH'] = str(output / 'nonexistent global modules')
env['SLINT_BACKEND'] = 'winit-software'
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def exercise(label, environment):
    reservations = []
    ports = []
    for _ in range(2):
        reservation = socket.socket()
        reservation.bind(('127.0.0.1', 0))
        ports.append(reservation.getsockname()[1])
        reservations.append(reservation)
    profile = {'schema_version': 1, 'name': '按需运行 · UI 程序包验证', 'mode': 'local', 'values': {
        'executable': 'X:/old/runtime/this-must-be-ignored.exe',
        'model_name': 'tiny-add', 'model_path': str(bundle / 'examples/tiny-add'),
        'rest_port': str(ports[0]), 'port': str(ports[1]), 'target_device': 'CPU', 'working_dir': ''}}
    profile_file = output / f'{label}-legacy-profile.json'
    profile_file.write_text(json.dumps(profile, ensure_ascii=False, indent=2), encoding='utf-8')
    for reservation in reservations:
        reservation.close()
    log_file = output / ('ui.log' if label == 'fresh' else 'cached-ui.log')
    capture = output / ('ui' if label == 'fresh' else 'cached-ui')
    inference = None
    ready_after = None
    started = time.monotonic()
    with log_file.open('w', encoding='utf-8') as log:
        process = subprocess.Popen(
            [str(bundle / 'OVMS-Studio.exe'), '--smoke-test', '--execution', 'native',
             '--profile', str(profile_file), '--capture-dir', str(capture)],
            cwd=unrelated, env=environment, stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = started + 1500
            while time.monotonic() < deadline and process.poll() is None:
                try:
                    with opener.open(f'http://127.0.0.1:{ports[0]}/v2/health/ready', timeout=0.5) as response:
                        ready = response.status == 200
                    if ready:
                        payload = {'inputs': [{'name': 'input', 'shape': [1, 3], 'datatype': 'FP32', 'data': [1, 2, 3]}]}
                        request = urllib.request.Request(f'http://127.0.0.1:{ports[0]}/v2/models/tiny-add/infer',
                            data=json.dumps(payload).encode(), headers={'Content-Type': 'application/json'})
                        with opener.open(request, timeout=5) as response:
                            inference = json.load(response)['outputs'][0]['data']
                        ready_after = time.monotonic() - started
                        print(f'{label}: first endpoint inference after {ready_after:.2f}s.', flush=True)
                        break
                except (OSError, ValueError):
                    time.sleep(0.15)
            code = process.wait(timeout=100)
        except BaseException:
            process.kill()
            process.wait()
            raise
    assert code == 0, log_file.read_text(encoding='utf-8', errors='replace')
    assert inference == [2, 4, 6], f'{label}: unexpected inference {inference}'
    ui = json.loads((capture / 'report.json').read_text(encoding='utf-8'))
    assert all(ui[k] for k in ['ready', 'restart_ready', 'configuration_callbacks', 'stopped_on_close'])
    assert ui['error'] is None
    return {'ui': ui, 'screenshots': str(capture), 'ready_after_seconds': round(ready_after, 2)}


assert not (data / 'native-current.json').exists()
fresh = exercise('fresh', env)
installation = json.loads((data / 'native-current.json').read_text(encoding='utf-8-sig'))
runtime = data / 'native' / installation['version'] / 'ovms'
assert runtime.is_dir() and not runtime.is_relative_to(bundle)
python_env = env.copy()
python_env['PYTHONHOME'] = str(runtime / 'python')
python_env.pop('PYTHONPATH', None)
python_result = subprocess.run(
    [str(runtime / 'python/python.exe'), '-c',
     'import sys,json; print(json.dumps({"prefix":sys.prefix,"executable":sys.executable,"version":sys.version}))'],
    cwd=unrelated, env=python_env, capture_output=True, text=True,
    encoding='utf-8', errors='replace', timeout=30, check=True)
python_info = json.loads(python_result.stdout)
assert Path(python_info['prefix']).resolve() == (runtime / 'python').resolve()

offline = {key: value for key, value in env.items() if key.lower() not in {'http_proxy', 'https_proxy', 'all_proxy', 'no_proxy'}}
offline.update({'HTTP_PROXY': 'http://127.0.0.1:9', 'HTTPS_PROXY': 'http://127.0.0.1:9',
                'ALL_PROXY': 'http://127.0.0.1:9', 'NO_PROXY': '127.0.0.1,localhost'})
cached = exercise('cached', offline)
blocked = subprocess.run([str(bundle / 'OVMS-Studio.exe'), '--prepare-runtime', '--execution', 'native', '--no-persist'],
    cwd=unrelated, env=offline, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=60)
assert blocked.returncode != 0 and '无法检查 OVMS 最新版本' in blocked.stderr, 'Offline proxy did not block the latest-version check'
assert json.loads((data / 'native-current.json').read_text(encoding='utf-8-sig')) == installation
result = {'bundle': str(bundle), 'ui_only': True, 'clean_path': True,
          'unrelated_working_directory': True, 'legacy_runtime_path_ignored': True,
          'managed_download_verified': True, 'cached_offline_reuse': True,
          'installation': installation, 'downloaded_python': python_info, 'inference': [2, 4, 6],
          'ui': fresh['ui'], 'screenshots': fresh['screenshots'],
          'fresh_ready_after_seconds': fresh['ready_after_seconds'],
          'cached_ready_after_seconds': cached['ready_after_seconds']}
(output / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
(project / 'work/bundle-check-result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
print('UI-only package, latest native download, inference, UI lifecycle and offline cache reuse passed.', flush=True)
print('Evidence:', output, flush=True)
