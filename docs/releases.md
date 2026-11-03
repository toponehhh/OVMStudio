# 自动构建与版本发布

仓库：[toponehhh/OVMStudio](https://github.com/toponehhh/OVMStudio)。

推送 `main`、提交针对 `main` 的 PR、推送 `v*` 标签或手动运行工作流时，GitHub Actions 会运行 Windows UI/本机验证和 Linux Docker 验证。

## 发布新版本

1. 修改 `Cargo.toml` 的应用版本，并更新 `Cargo.lock` 中对应的版本。
2. 将提交推送到 `main`。
3. 检查通过后，工作流会为尚未发布的版本自动创建 `v版本号` 标签，并上传 ZIP 和 SHA-256 文件到 Release。

也可以手动推送 `v*` 标签。标签必须与该提交的 `Cargo.toml` / `Cargo.lock` 版本一致。版本包含 `-beta` 等预发布标识时，Release 会标记为 prerelease。

同一版本后续提交仍会运行检查并产生构建附件，但不会覆盖已经公开的 Release。若版本标签已指向别的提交、手动创建的草稿与本工作流不匹配，或已发布版本缺失附件，发布会停止，保留原有内容。

## 工作流做什么

- 使用固定 Rust 工具链和已锁定的 Cargo 依赖。
- 固定版本 OVMS 仅用于参数契约和本机测试，绝不打入发布包。
- 运行格式、Clippy、发布脚本测试、参数契约和真实 OVMS 测试。
- 在精简 PATH、独立工作目录和空缓存下运行 UI，自动下载最新稳定 OVMS，执行真实推理、停止/重启和退出回收验证。
- 验证旧可执行文件路径被忽略、下载后 Python 可用，以及断网后复用已安装缓存。
- 独立 Linux job 检查 Docker、拉取官方镜像，验证中文/空格/逗号路径、推理、配置转换、停止/重启、失败和容器清理。
- 只把 UI、示例、文档和许可证压缩，校验 ZIP CRC；发布校验会拒绝包含 OVMS 运行文件的程序包。
- 根据当前 CI 实际证据生成验证报告和截图。
- 发布 job 依赖 Windows 与 Docker 两个 job，重新校验附件和 UI 的 SHA-256；先上传到草稿，附件齐全后公开。草稿通过 CLI 查询数据库 ID，避免按已发布标签索引查询草稿。

只使用 GitHub 自动提供的 `GITHUB_TOKEN`，不需要额外配置个人 PAT。构建 job 为只读；只有独立发布 job 请求 `contents: write`。PR 不发布版本。

## 查看结果

- [工作流与日志](https://github.com/toponehhh/OVMStudio/actions/workflows/release.yml)
- [正式版本与下载](https://github.com/toponehhh/OVMStudio/releases)
- Actions 附件保留完整 ZIP、校验文件和验证证据；Release 附件用于长期下载。

上传到 Git 仓库的是源码、文档及小型示例。OVMS 运行包和 Docker 镜像由用户端按需下载，不放入 Git 历史或 Release ZIP。
