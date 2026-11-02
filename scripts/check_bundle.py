"""Exercise the release UI with only its bundled OVMS/Python and a clean PATH."""
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
import uuid

project = Path(__file__).resolve().parent.parent
bundle = Path(sys.argv[1]).resolve()
output = project / 'work' / ('bundle-check-' + uuid.uuid4().hex)
output.mkdir(parents=True)
unrelated = output / 'unrelated working directory'
unrelated.mkdir()
windows = Path(os.environ['SystemRoot'])
env = {k: v for k, v in os.environ.items() if k.upper() not in
       {'PATH', 'PYTHONHOME', 'PYTHONPATH', 'OVMS_HOME', 'OVMS_MODEL_REPOSITORY_PATH'}}
env['PATH'] = str(windows / 'System32') + os.pathsep + str(windows)
env['OVMS_HOME'] = str(output / 'nonexistent global runtime')
env['PYTHONHOME'] = str(output / 'nonexistent global Python')
env['PYTHONPATH'] = str(output / 'nonexistent global modules')
env['SLINT_BACKEND'] = 'winit-software'

python_env = env.copy()
python_env['PYTHONHOME'] = str(bundle / 'ovms/python')
python_env.pop('PYTHONPATH', None)
python_result = subprocess.run(
    [str(bundle / 'ovms/python/python.exe'), '-c',
     'import sys,json; print(json.dumps({"prefix":sys.prefix,"executable":sys.executable,"version":sys.version}))'],
    cwd=unrelated, env=python_env, capture_output=True, text=True,
    encoding='utf-8', errors='replace', timeout=20, check=True)
python_info = json.loads(python_result.stdout)
assert Path(python_info['prefix']).resolve() == (bundle / 'ovms/python').resolve()

reservations = []
ports = []
for _ in range(2):
    reservation = socket.socket()
    reservation.bind(('127.0.0.1', 0))
    ports.append(reservation.getsockname()[1])
    reservations.append(reservation)
profile = {'schema_version': 1, 'name': '内置 OVMS · 完整程序包验证', 'mode': 'local', 'values': {
    'executable': 'X:/old/runtime/this-must-be-ignored.exe',
    'model_name': 'tiny-add', 'model_path': str(bundle / 'examples/tiny-add'),
    'rest_port': str(ports[0]), 'port': str(ports[1]), 'target_device': 'CPU', 'working_dir': ''}}
profile_file = output / 'legacy-profile.json'
profile_file.write_text(json.dumps(profile, ensure_ascii=False, indent=2), encoding='utf-8')
for reservation in reservations:
    reservation.close()

log_file = output / 'ui.log'
capture = output / 'ui'
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
inference = None
# A cold hosted Windows runner can spend over 100 seconds initializing the UI
# before it starts OVMS. Keep polling through that cold
# start; the UI still has its own 70-second server/lifecycle deadline.
startup_timeout = 180
completion_timeout = 85
started = time.monotonic()
ready_after = None
with log_file.open('w', encoding='utf-8') as log:
    process = subprocess.Popen(
        [str(bundle / 'OVMS-Studio.exe'), '--smoke-test', '--profile', str(profile_file), '--capture-dir', str(capture)],
        cwd=unrelated, env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        deadline = started + startup_timeout
        while time.monotonic() < deadline and process.poll() is None:
            try:
                with opener.open(f'http://127.0.0.1:{ports[0]}/v2/health/ready', timeout=0.5) as response:
                    ready = response.status == 200
                if ready:
                    data = json.dumps({'inputs': [{'name': 'input', 'shape': [1, 3], 'datatype': 'FP32', 'data': [1, 2, 3]}]}).encode()
                    request = urllib.request.Request(f'http://127.0.0.1:{ports[0]}/v2/models/tiny-add/infer',
                                                     data=data, headers={'Content-Type': 'application/json'})
                    with opener.open(request, timeout=5) as response:
                        inference = json.load(response)['outputs'][0]['data']
                    ready_after = time.monotonic() - started
                    print(f'First endpoint inference after {ready_after:.2f}s.', flush=True)
                    break
            except (OSError, ValueError):
                time.sleep(0.15)
        code = process.wait(timeout=completion_timeout)
    except BaseException:
        process.kill()
        process.wait()
        raise
assert code == 0, log_file.read_text(encoding='utf-8', errors='replace')
assert inference == [2, 4, 6], f'Inference was {inference}; startup budget was {startup_timeout}s'
ui = json.loads((capture / 'report.json').read_text(encoding='utf-8'))
assert all(ui[k] for k in ['ready', 'restart_ready', 'configuration_callbacks', 'stopped_on_close'])
assert ui['error'] is None

# A standalone GUI must fail, even when a valid external runtime is offered by
# PATH, OVMS_HOME and a legacy profile. This prevents hidden machine dependencies.
missing = output / 'missing runtime application'
missing.mkdir()
shutil.copy2(bundle / 'OVMS-Studio.exe', missing / 'OVMS-Studio.exe')
negative_env = env.copy()
negative_env['PATH'] = str(bundle / 'ovms') + os.pathsep + env['PATH']
negative_env['OVMS_HOME'] = str(bundle / 'ovms')
profile['values']['executable'] = str(bundle / 'ovms/ovms.exe')
profile_file.write_text(json.dumps(profile, ensure_ascii=False), encoding='utf-8')
negative_capture = output / 'missing-runtime-ui'
negative = subprocess.run([str(missing / 'OVMS-Studio.exe'), '--smoke-test', '--profile', str(profile_file),
                           '--capture-dir', str(negative_capture)], cwd=unrelated, env=negative_env,
                          capture_output=True, text=True, encoding='utf-8', errors='replace',
                          timeout=startup_timeout + completion_timeout)
negative_report = json.loads((negative_capture / 'report.json').read_text(encoding='utf-8'))
assert negative.returncode != 0
assert not negative_report['ready']
assert '内置 OVMS 缺失' in negative_report['error']
result = {'bundle': str(bundle), 'clean_path': True, 'unrelated_working_directory': True,
          'legacy_runtime_path_ignored': True, 'bundled_python': python_info,
          'inference': inference, 'ui': ui, 'missing_runtime_rejected': True,
          'endpoint_ready_after_seconds': round(ready_after, 2),
          'verification_elapsed_seconds': round(time.monotonic() - started, 2),
          'screenshots': str(capture)}
(output / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
(project / 'work/bundle-check-result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
print('Bundled runtime, clean PATH, relocation, inference and missing-runtime checks passed.', flush=True)
print('Evidence:', output, flush=True)
