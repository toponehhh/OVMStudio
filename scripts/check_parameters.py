"""Check every UI CLI flag against the selected official OVMS binary's help."""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

root = Path(__file__).resolve().parent.parent
binary = Path(sys.argv[1] if len(sys.argv) > 1 else root / "work/runtime-python-on/ovms/ovms.exe").resolve()
env = os.environ.copy()
env["PATH"] = str(binary.parent) + os.pathsep + str(binary.parent / 'python') + os.pathsep + env.get("PATH", "")
if (binary.parent / 'python').is_dir():
    env['PYTHONHOME'] = str(binary.parent / 'python')
    env.pop('PYTHONPATH', None)
tasks = ["", "text_generation", "embeddings", "rerank", "image_generation", "text2speech", "speech2text"]
supported = set()
for task in tasks:
    args = [str(binary), "--help"] + ([f"--task={task}"] if task else [])
    output = subprocess.run(args, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=15, check=True)
    supported.update(re.findall(r"--([A-Za-z0-9_-]+)", output.stdout + output.stderr))
fields = re.findall(r'field!\s*\(\s*"([A-Za-z0-9_-]+)"', (root / "src/config.rs").read_text(encoding="utf-8"))
internal = {"working_dir", "extra_args", "environment"}
flags = set(fields) - internal
missing = sorted(flags - supported)
report = {"binary": str(binary), "ui_flags": len(flags), "checked_tasks": tasks[1:], "unsupported": missing}
(root / "work/parameter-check.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(report, ensure_ascii=False, indent=2))
if missing:
    raise SystemExit(1)
