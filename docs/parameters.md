# OVMS 2026.4.1 启动参数核对

核对日期：2026-11-02。按 GitHub `releases/latest` 查询，最新稳定发布为 `v2026.4.1`。实现绑定这个发布标签，避免把主分支中尚未发布的选项放入默认命令。

## 证据来源

- [Intel 官方参数文档](https://docs.openvino.ai/2026/model-server/ovms_docs_parameters.html)：通过本机 ungoogled Chromium 渲染读取，核对真实页面正文。
- [发布标签中的参数文档](https://github.com/openvinotoolkit/model_server/blob/v2026.4.1/docs/parameters.md)：保留参数所属版本的边界。
- [CLI 解析器](https://github.com/openvinotoolkit/model_server/blob/v2026.4.1/src/cli_parser.cpp)：参数名、类型、默认值与运行模式。
- [文本生成参数解析器](https://github.com/openvinotoolkit/model_server/blob/v2026.4.1/src/graph_export/graph_cli_parser.cpp)。
- [Embedding 参数解析器](https://github.com/openvinotoolkit/model_server/blob/v2026.4.1/src/graph_export/embeddings_graph_cli_parser.cpp)。
- [图像生成参数解析器](https://github.com/openvinotoolkit/model_server/blob/v2026.4.1/src/graph_export/image_generation_graph_cli_parser.cpp)。
- [官方裸机安装与环境说明](https://github.com/openvinotoolkit/model_server/blob/v2026.4.1/docs/deploying_server_baremetal.md)。
- 0.1.1 一体包使用官方 Windows `python_on` 运行时，按官方发布的 SHA-256 校验。初版验证使用 `python_off`；两者 CLI 版本均为 `2026.4.1`。

## 运行模式与生成规则

| UI 模式 | 必需参数 | 会省略的参数 |
|---|---|---|
| 本地模型 | `model_name`、`model_path` | `config_path`、`source_model`、HF 下载参数 |
| HF 下载并服务 | `model_name`、`source_model`、`model_repository_path` | `model_path`、`config_path`、`pull` |
| 多模型配置 | `config_path` | 所有单模型和任务专用参数，由文件配置 |

`--pull` 仅下载后退出；`--configure` 仅生成 / 更新 graph 配置。`list_models`、`add_to_config`、`remove_from_config` 属于配置管理。本 UI 的「启动」不会执行这些命令，扩展参数也不能改变这一语义。多模型文件由用户自行维护，UI 会校验其为合法 JSON 对象，再交给 OVMS 进行完整语义校验。

## 参数覆盖

| 界面 | 参数 |
|---|---|
| 模型与来源 | `model_name`、`model_path`、`config_path`、`source_model`、`model_repository_path`、`task`、`weight-format`；OVMS 随应用内置，没有运行时路径选项 |
| 服务与网络 | `rest_port`、`port`、`rest_bind_address`、`grpc_bind_address`、`rest_workers`、`grpc_workers`、`metrics_enable`、`metrics_list` |
| 通用推理 | `target_device`、`plugin_config` |
| 传统推理 | `nireq`、`batch_size`、`shape`、`layout`、`model_version_policy` |
| 文本生成 | `max_num_seqs`、`cache_size`、`max_num_batched_tokens`、`pipeline_type`、`enable_prefix_caching`、`dynamic_split_fuse` |
| Embedding | `num_streams`、`normalize`、`truncate`、`pooling`、`max_length` |
| Rerank | `num_streams`、`max_allowed_chunks` |
| 图像生成 | `num_streams`、`resolution`、`max_resolution`、`default_resolution`、`max_num_images_per_prompt`、`num_images_per_prompt`、`guidance_scale`、`default_num_inference_steps`、`max_num_inference_steps`、`source_loras` |
| 语音 | `num_streams`；TTS 另有 `model_type` |
| 日志与缓存 | `cache_dir`、`log_level`、`log_path`、`file_system_poll_wait_seconds`、`idle_unload_timeout_seconds` |
| 网络进阶 | `grpc_max_threads`、`grpc_memory_quota`、`grpc_channel_arguments` |
| CORS 与访问 | `allowed_origins`、`allowed_methods`、`allowed_headers`、`allow_credentials`、`api_key_file` |
| 媒体 | `allowed_media_domains`、`allowed_local_media_path` |
| 预处理与扩展 | `mean`、`scale`、`color_format`、`precision`、`cpu_extension`、`custom_node_resources_cleaner_interval_seconds`、`verbose_response`、`disable_input_count_validation` |
| 文本生成进阶 | `draft_model_path`、`draft_source_model`、`draft_device`、`max_prompt_len`、`kv_cache_precision`、`model_distribution_policy`、`reasoning_parser`、`tool_parser`、`enable_tool_guided_generation`、`cache_interval_multiplier` |
| 下载进阶 | `gguf_filename`、`extra_quantization_params` |
| 扩展 | 非空的用户 JSON 环境变量对象、JSON 参数数组 |

## 需要特殊处理的地方

1. **启动默认值**：OVMS 两个端口默认都是 `0`。UI 为快速使用明确提供 REST `8000`、gRPC `9000` 和本机监听地址。至少启用一个协议，两个端口必须不同。
2. **设备**：OVMS 默认设备可自动检测；UI 初始明确选择 `CPU`，便于在没有加速器的机器上启动。清空设备项会使用 OVMS 自动检测，`AUTO` 字面值则是 OpenVINO 的设备模式。
3. **布尔参数**：统一用 `--name=true/false`，避免 cxxopts 的隐式布尔值把后续参数错误识别为值。任务切换时保留填写内容，但只传当前任务的参数。
4. **批大小 / 形状**：非空 `shape` 会覆盖 `batch_size`。预览与实际命令都省略被覆盖的批大小。
5. **Embedding 拼写**：文档表中的 `max_legth` 有拼写错误。发布源码和官方二进制 `--help --task=embeddings` 使用 **`max_length`**。
6. **KV Cache**：`cache_size` 的发布 CLI 类型为 `uint32_t`，不是任意浮点数；`0` 表示动态分配。界面拒绝小数。
7. **图像 Guidance scale**：CLI 类型为 float，支持 `7.5`。静态图片数与 Guidance scale 需要单一 `resolution`。
8. **清理间隔**：文档部分描述写默认 `1`，CLI 实际默认 `300`；以对应发布源码为准。
9. **API Key**：只保存 `api_key_file` 的路径，不读取文件内容。密钥或 `API_KEY` 的实际行为由内置 OVMS 版本决定。
10. **内置运行时与环境**：发布版只加载 UI 旁的 `ovms/ovms.exe`。旧配置中的 `executable` 会移除，不搜索 PATH、`OVMS_HOME` 或开发目录。Windows 参照官方 `setupvars.bat` 配置随包提供的 DLL 和 Python；清除继承的 `PYTHONHOME`、`PYTHONPATH`，再设置内置环境。清除继承的 `OVMS_MODEL_REPOSITORY_PATH`，避免默认配置文件影响启动模式；自定义环境中的显式设置仍可覆盖。
11. **就绪检查**：只在后台子进程启动后探测所配置 REST 地址的 `/v2/health/ready`，HTTP 200 才标记「服务已就绪」。启动前先检查端口占用，避免误把已有服务当作这次启动的服务。端口检查与 OVMS 实际 bind 之间仍有通常的时间窗口；最终启动失败由退出状态和日志呈现。
12. **扩展与版本差异**：`trace_path` 只在启用对应编译选项的二进制中存在，可在核对内置运行时帮助后通过扩展参数传入。主分支后续新增的参数不自动假定在 2026.4.1 中可用；升级运行时应重新构建与验证完整程序包。

本工具生成启动命令，完整的模型、设备、插件参数兼容性仍由实际 OVMS 验证。程序存在、端口开放、本地测试通过与生成式模型在特定硬件上可用，是不同的验证结论。
