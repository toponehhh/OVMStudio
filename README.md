# OVMS Studio

[![Build and Release](https://github.com/toponehhh/OVMStudio/actions/workflows/release.yml/badge.svg)](https://github.com/toponehhh/OVMStudio/actions/workflows/release.yml)

[下载 Windows UI 程序包](https://github.com/toponehhh/OVMStudio/releases/latest) · [自动发布说明](docs/releases.md)

Rust + Slint 编写的 Intel OpenVINO Model Server 桌面启动器，提供中文参数界面、Endpoint、日志和进程管理。

**0.2.0 起发布包只包含 UI，不包含 OVMS 运行包。** 在“设置”中选择本机或 Docker，应用负责准备运行环境，无需指定 OVMS 可执行文件路径。

![服务就绪](docs/screenshots/server-ready.png)

## 开始使用

1. 下载并解压 ZIP，运行 `OVMS-Studio/OVMS-Studio.exe`。
2. 在“设置”中选择运行方式，也可以点击准备按钮提前下载。
3. 选择本地模型、Hugging Face 仓库或多模型 `config.json`，调整设备、端口和推理参数。
4. 点击“启动服务”。首次使用自动准备环境；显示“服务已就绪”表示实际健康检查返回 HTTP 200。

| 运行方式 | 准备过程 | 后续启动 |
|---|---|---|
| 本机 | 查询 Intel 官方最新稳定 Release，下载 Windows x64 包，校验 SHA-256 并解压到用户缓存 | 复用已下载的运行时，自动设置子进程 DLL 和 Python 环境 |
| Docker | 检查客户端、本机引擎及 Linux 容器模式，再拉取最新稳定版 `openvino/model_server` 官方镜像（按设备选择 CPU 或 `-gpu`） | 验证引擎和已拉取镜像，自动挂载模型并映射端口 |

“下载 / 更新最新 OVMS”或“检查 Docker 并拉取最新镜像”会重新检查官方最新稳定版。正常启动复用已准备的环境，本机缓存可离线使用。下载和拉取在后台执行，支持取消。

![运行设置](docs/screenshots/runtime-settings.png)

本机模式目前支持 Windows x64，官方包内的 Python 也会随下载获取，无需另外指定 Python。用户数据目录由平台规则确定，可通过 `OVMSTUDIO_DATA_DIR` 为便携部署或测试指定独立缓存目录。

Docker 模式需要已安装并启动的 Docker Engine；Windows 用户通常使用 Docker Desktop 的 Linux 容器模式。应用提供安装文档链接，不自动安装 Docker。显式 GPU/NPU 目标会选择 `-gpu` 镜像并生成设备透传参数。Linux GPU 使用 `/dev/dri`，NPU 使用 `/dev/accel`；Windows/WSL GPU 使用 `/dev/dxg` 和 WSL 驱动目录，Windows NPU 请使用本机模式。加速器需要对应宿主驱动，硬件路径未在 CPU-only CI 上实测。不支持远程 Docker context 的本机文件挂载。

## 模型和参数

| 来源 | 填写内容 |
|---|---|
| 本地传统模型 | 模型名称及含数字版本子目录的仓库，或支持的单个 `.onnx`、`.xml` 等文件 |
| 本地生成式模型 | 名称、导出目录及 `text_generation`、`embeddings` 等任务 |
| Hugging Face | 仓库 ID、模型名称、下载目录和任务；应用使用下载并服务模式，不使用仅下载的 `--pull` |
| 多模型配置 | 现有 `config.json`；模型路径、设备和推理设置由各模型定义 |

所有非必填参数留空时沿用 OVMS 默认值。传统模型的 `shape` 优先于 `batch_size`。已导出的 `OpenVINO/...` HF 模型不设置导出权重精度；需转换的模型可设置。参数研究基线见 [参数说明](docs/parameters.md)，实际运行时帮助可在界面中读取。

默认只监听本机 `127.0.0.1`，REST `8000`、gRPC `9000`；端口为 `0` 时关闭该协议。需要局域网访问时修改监听地址。仅启用 gRPC 时会显示“进程运行中，未验证模型就绪”。

Docker 会把宿主机路径转换成容器路径，模型目录只读挂载，缓存、日志和下载目录可写挂载。多模型 JSON 会生成转换后的临时副本，原文件保持不变；其中相对路径按所选工作目录解析。宿主机监听地址只用于端口发布，容器内监听地址自动设为 `0.0.0.0`。

## 配置、日志与退出

- 运行方式是应用级设置，自动保存；模型配置可命名保存并导入/导出，切换模型配置不改变运行方式。
- 导入配置中的相对文件路径以配置文件目录解析；手工输入的相对路径默认以 UI 所在目录为基准，可在高级参数中指定工作目录。旧配置的 `executable` 字段会忽略并移除。
- 参数逐项交给进程，不通过 Shell。扩展参数使用 JSON 字符串数组，不能覆盖专用字段或切换到一次性下载命令。
- 自定义环境 JSON 会明文保存。HF 令牌建议使用系统 `HF_TOKEN`；Docker 通过变量名转发令牌，不把值写入命令行或命令预览。
- 后台持续读取 stdout/stderr，界面保留最近 1,000 行，支持复制和导出。
- 本机进程由 Windows Job Object 管理；Docker 通过唯一容器名管理，停止、异常退出和关闭 UI 时显式清理容器。停止会中断正在进行的推理。

## 示例

导入 `examples/demo-profile.json` 后启动。示例模型计算 `output = input + input`：

```powershell
$body = '{"inputs":[{"name":"input","shape":[1,3],"datatype":"FP32","data":[1,2,3]}]}'
Invoke-RestMethod 'http://127.0.0.1:8000/v2/models/tiny-add/infer' -Method Post -ContentType 'application/json' -Body $body
```

预期输出 `[2,4,6]`。`examples/multi-model-config.json` 用于多模型模式。

## 构建与验证

需要 Rust 1.92+；CI 固定 Rust 1.99.1，Slint 1.18.1，Winit + software renderer。Windows 常规构建需要 Visual Studio C++ Build Tools：

```powershell
cargo build --locked --release
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
./scripts/package.ps1
```

本工作目录的便携 GNU 工具链可使用 `scripts/build-local.ps1`。`scripts/env-local.ps1` 的路径只用于本机开发，不是用户运行要求。

`package.ps1` 只复制 UI、示例、文档和许可证，生成 ZIP、清单和 SHA-256。打包不要求下载 OVMS；`scripts/fetch-runtime.py` 下载的固定版本仅是可复现的本机测试夹具，不会进入 ZIP。

```powershell
python scripts/fetch-runtime.py
$env:OVMS_TEST_BUNDLE = "$PWD/work/runtime-python-on"
cargo test --locked --test real_ovms -- --ignored --test-threads=1
```

真实 Docker 测试在已安装 Docker 的 Linux 主机运行，不需要编译 Slint：

```sh
cargo test --locked --no-default-features --test real_docker -- --ignored --test-threads=1 --nocapture
```

该测试拉取官方最新镜像，检查含空格、中文及逗号的模型目录，执行推理、停止、重启、多模型配置、无效模型失败和容器清理。Actions 要求 Windows 本机/UI 验证和 Linux Docker 验证全部成功后才发布。

发布包的 `docs/verification.md` 和截图由该次 CI 的实际证据生成。GPU/NPU、真实大模型下载和生成式模型推理需要对应硬件与模型另行验证。

| 源码 | 职责 |
|---|---|
| `ui/app.slint`、`src/main.rs` | 设置、模型参数、日志、状态和后台任务 |
| `src/config.rs` | 模型配置、应用运行设置与参数规则 |
| `src/deployment.rs` | 最新版本查询、校验下载、缓存安装及取消 |
| `src/docker.rs` | Docker 检查、镜像拉取、路径/端口映射和容器清理 |
| `src/runtime.rs`、`src/process.rs` | 实际进程启动、监督与就绪探测 |

官方来源：[OVMS Releases](https://github.com/openvinotoolkit/model_server/releases)、[Docker 部署](https://github.com/openvinotoolkit/model_server/blob/main/docs/deploying_server_docker.md)、[本机部署](https://github.com/openvinotoolkit/model_server/blob/main/docs/deploying_server_baremetal.md)。
