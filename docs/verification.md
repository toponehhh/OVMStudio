# OVMS Studio 0.1.1 一体包验证

日期：2026-11-02。平台：Windows x64。验证对象是 UI、OVMS 和 Python 一起提供的完整应用目录。

## 交付内容

- `OVMS-Studio.exe`：Rust 1.99.1 GNU + Slint 1.18.1 构建，版本 0.1.1。
- `ovms/`：官方 OVMS 2026.4.1 `python_on` 包，含 DLL、Python 3.12.10、许可证及第三方许可证。
- `examples/`：Tiny Add ONNX 模型、单模型 UI 配置和多模型配置示例。
- `README.md`、`QUICKSTART.txt`、`docs/` 与 `bundle-manifest.json`。
- 发布 ZIP 同名的 `.zip.sha256` 提供最终压缩包校验值；打包脚本强制检查 ZIP 必需文件和全部 CRC。

GUI SHA-256：`5753b9331d5c7e3e0b5dfa0e7f8dd24fc07a75d1aa8185f17008c5d75188ca21`。

官方 OVMS 压缩包 SHA-256：`aa79a0ae3b858356ba550978e397a8e8876a9d2706ffe4041b1dfd3f32cb8ce4`，已对照官方发布值验证。

## 运行时约定

UI 不提供 OVMS 可执行文件路径选项。发布版只加载 UI 同目录下的 `ovms/ovms.exe`，不搜索系统 PATH、OVMS_HOME、旧配置或开发目录。配置升级移除旧 `executable` 字段。缺少内置文件时提示重新解压完整应用。默认相对路径基于 UI 所在目录。

## 已完成验证

| 检查 | 结果 |
|---|---|
| 发布构建 | 成功 |
| Clippy 全目标、warnings 视为错误 | 通过 |
| Rustfmt --check | 通过 |
| 参数与配置契约 | 16 项通过，含运行时固定位置、旧配置迁移、缺失运行时处理 |
| 官方 python_on 运行时 | 6 项真实服务测试通过 |
| CLI 参数对照 | 79 个参数全部可在内置二进制帮助中确认 |
| 完整应用目录包含空格 | UI 启动、停止、重启通过 |
| 从其他工作目录启动 | 通过 |
| PATH 仅包含 Windows 系统目录 | 通过，不依赖系统 OVMS 或 Python |
| 无效的全局 PYTHONHOME/PYTHONPATH | 不影响内置环境，验证通过 |
| 内置 Python | 实际启动成功，sys.prefix 指向应用内 ovms/python |
| 真实模型推理 | FP32 [1,3]，输入 [1,2,3]，输出 [2,4,6] |
| 旧配置中的运行时路径 | 忽略后仍使用内置 OVMS，验证通过 |
| 仅复制 GUI、缺少 ovms 子目录 | 即使 PATH、OVMS_HOME 和旧配置提供有效外部 OVMS，仍拒绝回退并提示完整解压 |
| UI 配置回调 | 命名保存/恢复、本地/HF/多模型、Embedding/LLM 模板切换通过 |
| UI 关闭与资源释放 | 就绪、停止、重启就绪和退出释放端口均通过 |
| 原生渲染截图 | 6 个主页面和 5 个模式/停止状态已检查 |

真实服务测试还覆盖多模型 JSON、带空格路径的单个 ONNX 文件、端口冲突拒绝、无效模型退出和 Embedding `--max_length` 参数。

![一体包运行界面](screenshots/server-ready.png)

[内置运行时版本](screenshots/runtime-help.png) · [实时日志](screenshots/logs.png) · [多模型配置](screenshots/multi-model.png) · [停止状态](screenshots/server-stopped.png)

## 复现

```powershell
python scripts/fetch-runtime.py
./scripts/build-local.ps1 -Release
./scripts/build-local.ps1 -Test
./scripts/build-local.ps1 -RealTest
./scripts/build-local.ps1 -Clippy
./scripts/package.ps1
python scripts/check_bundle.py dist/OVMS-Studio
```

`check_bundle.py` 使用实际发布程序、真实 OVMS、独立工作目录、精简 PATH 和旧配置路径进行验证。UI 验证通过同一套界面回调驱动，截图来自原生 Window::take_snapshot；不等同于操作系统鼠标逐一点击所有控件。

GPU/NPU 推理、HF 大模型下载、真实 LLM/Embedding/图像/语音模型推理和 Linux 实机运行仍未验证。此记录为 Windows 本机验证，不代表远程或生产部署。
