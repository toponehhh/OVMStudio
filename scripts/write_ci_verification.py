"""Generate release verification notes from the CI run's actual test artifacts."""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

root = Path(__file__).resolve().parent.parent
ci = root / 'work/ci'
proof = json.loads((root / 'work/bundle-check-result.json').read_text(encoding='utf-8'))
for key in ['ui_only', 'clean_path', 'unrelated_working_directory', 'legacy_runtime_path_ignored', 'managed_download_verified', 'cached_offline_reuse']:
    if not proof[key]:
        raise ValueError(f'Bundle verification failed: {key}')
if proof['inference'] != [2, 4, 6] or proof['ui']['error'] is not None:
    raise ValueError('Real inference or UI validation did not pass')
for key in ['ready', 'restart_ready', 'stopped_on_close', 'configuration_callbacks']:
    if not proof['ui'][key]:
        raise ValueError(f'UI validation did not pass: {key}')
counts = {}
for name in ['contracts', 'runtime', 'deployment']:
    text = (ci / (name + '.log')).read_text(encoding='utf-8-sig')
    passed = re.search(r'test result: ok\. (\d+) passed; 0 failed; 0 ignored', text)
    if passed is None or int(passed[1]) == 0:
        raise ValueError(f'Missing successful test evidence for {name}')
    counts[name] = int(passed[1])
bundle = Path(proof['bundle'])
manifest = json.loads((bundle / 'bundle-manifest.json').read_text(encoding='utf-8-sig'))
digest = hashlib.sha256((bundle / 'OVMS-Studio.exe').read_bytes()).hexdigest()
if digest != manifest['application_sha256']:
    raise ValueError('Verified GUI binary does not match the bundle manifest')
rust = subprocess.check_output(['rustc', '--version'], text=True, encoding='utf-8').strip()
repository = os.environ['GITHUB_REPOSITORY']
commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True, encoding='utf-8').strip()
run = os.environ['GITHUB_RUN_ID']
report = f'''# OVMS Studio {manifest['application_version']} GitHub Actions 验证

此记录来自 [GitHub Actions 构建](https://github.com/{repository}/actions/runs/{run})，
源代码提交为 [{commit}](https://github.com/{repository}/commit/{commit})。

- 平台：Windows x64，MSVC；Rust：`{rust}`。
- 发布包：仅 UI，不包含 OVMS 或 Python 运行包；支持本机和 Docker 两种运行方式。
- 本次从官方最新稳定版下载 OVMS {proof['installation']['version']}，运行时自带 Python。
- 格式检查与 Clippy：本工作流前置步骤通过后才会执行本步骤。
- 契约测试：{counts['contracts']} 项通过；真实 OVMS 测试：{counts['runtime']} 项通过；运行方式与安装测试：{counts['deployment']} 项通过。
- UI 程序包：精简 PATH、独立工作目录、忽略旧可执行文件路径、首次自动下载、下载后 Python、真实推理和断网缓存复用全部通过。
- 推理输入 `[1,2,3]`，实际输出 `[2,4,6]`。
- UI：服务就绪、停止、重启、退出回收和配置切换通过。

GUI SHA-256：`{digest}`。

本次按需下载的 OVMS 压缩包 SHA-256：`{proof['installation']['archive_sha256']}`。

最终 ZIP 的校验值见同名 `.zip.sha256`。发布前会再次校验应用 SHA-256，并拒绝误包含 OVMS 的发布包。

真实 Docker 镜像拉取、挂载、推理、启停和容器清理由同一次 Actions 的 Linux Docker job 验证；正式发布要求 Windows 和 Docker 两个 job 全部成功。

GPU/NPU、HF 大模型下载、真实生成式模型推理与 Linux 桌面 UI 不在此验证范围内。

![服务就绪](screenshots/server-ready.png)
'''
(root / 'docs/verification.md').write_text(report, encoding='utf-8')
screens = {'page-0.png': 'server-ready.png', 'page-1.png': 'network.png',
           'page-2.png': 'classic-inference.png', 'page-3.png': 'advanced.png',
           'page-4.png': 'logs.png', 'page-5.png': 'runtime-settings.png', 'page-6.png': 'runtime-help.png',
           'hugging-face.png': 'hugging-face.png', 'multi-model.png': 'multi-model.png',
           'embeddings.png': 'embeddings.png', 'text-generation.png': 'text-generation.png',
           'stopped.png': 'server-stopped.png'}
for source, destination in screens.items():
    shutil.copy2(Path(proof['screenshots']) / source, root / 'docs/screenshots' / destination)
print(f'Generated CI verification report: {counts}')
