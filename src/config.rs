use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const OVMS_VERSION: &str = "2026.4.1";
pub const DOCS_URL: &str =
    "https://github.com/openvinotoolkit/model_server/blob/v2026.4.1/docs/parameters.md";
pub fn runtime_program_relative() -> &'static str {
    if cfg!(windows) {
        "ovms/ovms.exe"
    } else {
        "ovms/bin/ovms"
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Local,
    HuggingFace,
    Config,
}

impl Mode {
    pub fn index(self) -> i32 {
        match self {
            Self::Local => 0,
            Self::HuggingFace => 1,
            Self::Config => 2,
        }
    }
    pub fn from_index(index: i32) -> Self {
        match index {
            1 => Self::HuggingFace,
            2 => Self::Config,
            _ => Self::Local,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Text,
    Choice,
    Bool,
    File,
    Folder,
    Model,
    Json,
}

impl Kind {
    pub fn index(self) -> i32 {
        match self {
            Self::Text => 0,
            Self::Choice => 1,
            Self::Bool => 2,
            Self::File => 3,
            Self::Folder => 4,
            Self::Model => 5,
            Self::Json => 6,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub placeholder: &'static str,
    pub kind: Kind,
    pub choices: &'static [&'static str],
    pub page: i32,
    pub group: &'static str,
    // "single" includes local and HF; a task name applies only to that task.
    pub scope: &'static str,
    pub default: &'static str,
    pub number_min: Option<u64>,
}

macro_rules! field {
    ($key:literal, $label:literal, $hint:literal, $placeholder:literal, $kind:ident, $page:literal, $group:literal, $scope:literal, $default:literal) => {
        Field {
            key: $key,
            label: $label,
            hint: $hint,
            placeholder: $placeholder,
            kind: Kind::$kind,
            choices: &[],
            page: $page,
            group: $group,
            scope: $scope,
            default: $default,
            number_min: None,
        }
    };
}

pub fn fields() -> Vec<Field> {
    let mut f = vec![
        field!(
            "working_dir",
            "相对路径基准目录",
            "留空使用 UI 所在目录。可为模型、缓存和日志指定相对路径的基准目录。",
            "可选，默认使用应用目录",
            Folder,
            3,
            "运行与日志",
            "all",
            ""
        ),
        field!(
            "model_name",
            "服务中的模型名称",
            "客户端使用此名称调用模型。切换来源后仍可复用。",
            "例如 qwen3 / resnet",
            Text,
            0,
            "模型来源",
            "single",
            "my-model"
        ),
        field!(
            "model_path",
            "本地模型路径",
            "传统模型选含数字版本目录的文件夹，也可选 .onnx / .xml；生成式模型选导出目录。",
            "选择模型目录或模型文件",
            Model,
            0,
            "模型来源",
            "local",
            ""
        ),
        field!(
            "source_model",
            "Hugging Face 仓库",
            "使用 --source_model 下载并启动服务；不会加入仅下载的 --pull。",
            "OpenVINO/Qwen3-0.6B-int4-ov",
            Text,
            0,
            "模型来源",
            "hf",
            ""
        ),
        field!(
            "model_repository_path",
            "模型下载目录",
            "模型文件会保留在此目录，后续启动可复用。",
            "选择本地模型仓库",
            Folder,
            0,
            "模型来源",
            "hf",
            ""
        ),
        field!(
            "weight-format",
            "导出权重精度",
            "仅用于需要 optimum 转换的原始模型；OpenVINO 已导出仓库应留空。",
            "留空使用 OVMS 默认值",
            Choice,
            0,
            "模型来源",
            "hf",
            ""
        ),
        field!(
            "config_path",
            "多模型配置文件",
            "读取已有 config.json；此模式不会传入 model_path、task 或设备等单模型参数。",
            "选择 config.json",
            File,
            0,
            "模型来源",
            "config",
            ""
        ),
        field!(
            "task",
            "模型任务",
            "传统推理或已有 graph.pbtxt 可留空；生成式任务请明确选择。",
            "传统推理 / 自动识别",
            Choice,
            0,
            "模型来源",
            "single",
            ""
        ),
        field!(
            "rest_port",
            "REST / HTTP 端口",
            "设为 0 关闭 HTTP；OpenAI 兼容接口和健康检查依赖此端口。",
            "8000",
            Text,
            1,
            "服务地址",
            "all",
            "8000"
        ),
        field!(
            "rest_bind_address",
            "REST 监听地址",
            "127.0.0.1 供本机访问；0.0.0.0 供其他机器访问。也支持逗号分隔的地址。",
            "127.0.0.1",
            Text,
            1,
            "服务地址",
            "all",
            "127.0.0.1"
        ),
        field!(
            "port",
            "gRPC 端口",
            "设为 0 关闭 gRPC；启用时应与 REST 使用不同端口。",
            "9000",
            Text,
            1,
            "服务地址",
            "all",
            "9000"
        ),
        field!(
            "grpc_bind_address",
            "gRPC 监听地址",
            "支持 IPv4、IPv6、主机名或逗号分隔的多个地址。",
            "127.0.0.1",
            Text,
            1,
            "服务地址",
            "all",
            "127.0.0.1"
        ),
        field!(
            "rest_workers",
            "REST 工作线程",
            "留空由 OVMS 按 CPU 数量自动选择。",
            "自动",
            Text,
            1,
            "并发与观测",
            "all",
            ""
        ),
        field!(
            "grpc_workers",
            "gRPC 服务实例",
            "通常使用 1；高并发场景可调高，不应超过 CPU 核数。",
            "1",
            Text,
            1,
            "并发与观测",
            "all",
            ""
        ),
        field!(
            "metrics_enable",
            "启用 Prometheus 指标",
            "通过 REST 的 /metrics 暴露指标，需要启用 REST。",
            "",
            Bool,
            1,
            "并发与观测",
            "all",
            "false"
        ),
        field!(
            "metrics_list",
            "指标列表",
            "逗号分隔；留空使用默认指标。",
            "ovms_requests_success,ovms_requests_fail",
            Text,
            1,
            "并发与观测",
            "all",
            ""
        ),
        field!(
            "target_device",
            "推理设备",
            "CPU / GPU / NPU，或填写 GPU.0、AUTO:GPU,CPU 等设备表达式。",
            "留空由 OVMS 自动选择",
            Text,
            2,
            "推理引擎",
            "single",
            "CPU"
        ),
        field!(
            "plugin_config",
            "OpenVINO 插件配置",
            "JSON 对象，例如 {\"PERFORMANCE_HINT\":\"LATENCY\"}。",
            "{}",
            Json,
            2,
            "推理引擎",
            "single",
            ""
        ),
        field!(
            "nireq",
            "推理请求队列",
            "0 或留空使用 OpenVINO 自动计算的队列大小。",
            "自动",
            Text,
            2,
            "传统模型",
            "classic",
            ""
        ),
        field!(
            "batch_size",
            "批大小",
            "正整数或 auto；设置 shape 时 OVMS 会忽略此值，预览也会省略。",
            "继承模型 / auto / 1",
            Text,
            2,
            "传统模型",
            "classic",
            ""
        ),
        field!(
            "shape",
            "输入形状",
            "支持 auto、(1,3,224,224) 或按输入名称指定的 JSON 对象。",
            "继承模型",
            Text,
            2,
            "传统模型",
            "classic",
            ""
        ),
        field!(
            "layout",
            "张量布局",
            "例如 NHWC:NCHW，或按输入输出名称指定的 JSON 对象。",
            "继承模型",
            Text,
            2,
            "传统模型",
            "classic",
            ""
        ),
        field!(
            "model_version_policy",
            "模型版本策略",
            "JSON：{\"latest\":{\"num_versions\":1}}、{\"all\":{}} 或 specific。",
            "默认使用最新版本",
            Json,
            2,
            "传统模型",
            "classic",
            ""
        ),
        field!(
            "max_num_seqs",
            "最大并发序列",
            "Continuous batching 同时处理的最大序列数。OVMS 默认 256。",
            "256",
            Text,
            2,
            "文本生成",
            "text_generation",
            ""
        ),
        field!(
            "cache_size",
            "KV Cache 大小 / GB",
            "0 使用动态分配；此处必须为非负整数。",
            "0 · 动态分配",
            Text,
            2,
            "文本生成",
            "text_generation",
            ""
        ),
        field!(
            "max_num_batched_tokens",
            "每批最大 Token 数",
            "留空使用运行时默认策略。",
            "自动",
            Text,
            2,
            "文本生成",
            "text_generation",
            ""
        ),
        field!(
            "pipeline_type",
            "生成管线",
            "LM / VLM 为基础管线；CB 为 continuous batching。",
            "AUTO",
            Choice,
            2,
            "文本生成",
            "text_generation",
            ""
        ),
        field!(
            "enable_prefix_caching",
            "复用前缀缓存",
            "缓存重复提示词的 Token。OVMS 默认开启。",
            "",
            Bool,
            2,
            "文本生成",
            "text_generation",
            "true"
        ),
        field!(
            "dynamic_split_fuse",
            "动态 Split Fuse",
            "动态组合 prefill 与 decode 阶段。OVMS 默认开启。",
            "",
            Bool,
            2,
            "文本生成",
            "text_generation",
            "true"
        ),
        field!(
            "num_streams",
            "并行执行流",
            "适用于 Embeddings、Rerank、图像和语音任务；留空使用运行时默认值。",
            "自动",
            Text,
            2,
            "任务参数",
            "streams",
            ""
        ),
        field!(
            "normalize",
            "归一化向量",
            "将生成的 embedding 归一化。",
            "",
            Bool,
            2,
            "向量模型",
            "embeddings",
            "true"
        ),
        field!(
            "truncate",
            "自动截断输入",
            "超过模型上下文长度时截断。",
            "",
            Bool,
            2,
            "向量模型",
            "embeddings",
            "false"
        ),
        field!(
            "pooling",
            "Pooling 策略",
            "留空由模型配置自动检测。",
            "自动",
            Choice,
            2,
            "向量模型",
            "embeddings",
            ""
        ),
        field!(
            "max_length",
            "最大输入 Token 数",
            "使用源码中的 max_length；官方参数表存在 max_legth 的拼写错误。",
            "继承模型",
            Text,
            2,
            "向量模型",
            "embeddings",
            ""
        ),
        field!(
            "max_allowed_chunks",
            "最大文本块数",
            "Rerank 请求可包含的最大 chunks 数。默认 10000。",
            "10000",
            Text,
            2,
            "重排序",
            "rerank",
            ""
        ),
        field!(
            "resolution",
            "允许的图像分辨率",
            "以空格分隔，例如 512x512 1024x1024；单个分辨率启用静态管线。",
            "继承模型",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "max_resolution",
            "最大分辨率",
            "格式 WxH，例如 1024x1024。",
            "继承模型",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "default_resolution",
            "默认分辨率",
            "客户端未指定时使用；格式 WxH。",
            "继承模型",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "max_num_images_per_prompt",
            "每条提示词最大图片数",
            "限制客户端请求的图片数量。",
            "继承模型",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "num_images_per_prompt",
            "静态管线图片数",
            "仅适用于指定单个 resolution 的静态管线。",
            "继承模型",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "guidance_scale",
            "静态管线 Guidance scale",
            "仅适用于指定单个 resolution 的静态管线。支持小数。",
            "7.5",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "default_num_inference_steps",
            "默认推理步数",
            "客户端未指定步数时使用。",
            "继承模型",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "max_num_inference_steps",
            "最大推理步数",
            "客户端可请求的最大步数。",
            "继承模型",
            Text,
            2,
            "图像生成",
            "image_generation",
            ""
        ),
        field!(
            "model_type",
            "语音合成模型类型",
            "kokoro 为推荐类型；speecht5 为旧版类型。",
            "kokoro",
            Choice,
            2,
            "语音合成",
            "text2speech",
            ""
        ),
        field!(
            "cache_dir",
            "编译缓存目录",
            "复用 OpenVINO 编译缓存，加快后续模型加载。",
            "可选",
            Folder,
            3,
            "运行与日志",
            "all",
            ""
        ),
        field!(
            "log_level",
            "日志级别",
            "TRACE / DEBUG / INFO / WARNING / ERROR。",
            "INFO",
            Choice,
            3,
            "运行与日志",
            "all",
            "INFO"
        ),
        field!(
            "log_path",
            "OVMS 日志文件",
            "可选；界面同时收集 stdout / stderr。",
            "可选，完整文件路径",
            Text,
            3,
            "运行与日志",
            "all",
            ""
        ),
        field!(
            "file_system_poll_wait_seconds",
            "模型与配置轮询间隔 / 秒",
            "默认 1；设为 0 关闭变更检测。",
            "1",
            Text,
            3,
            "运行与日志",
            "all",
            ""
        ),
        field!(
            "idle_unload_timeout_seconds",
            "空闲卸载时间 / 秒",
            "仅适用于 config.json 多模型模式；0 关闭。",
            "0",
            Text,
            3,
            "运行与日志",
            "config",
            ""
        ),
        field!(
            "grpc_max_threads",
            "gRPC 最大线程",
            "留空根据 CPU 数量设置。",
            "自动",
            Text,
            3,
            "网络进阶",
            "all",
            ""
        ),
        field!(
            "grpc_memory_quota",
            "gRPC 缓冲区配额 / 字节",
            "默认 2147483648（2 GB）。",
            "2147483648",
            Text,
            3,
            "网络进阶",
            "all",
            ""
        ),
        field!(
            "grpc_channel_arguments",
            "gRPC Channel 参数",
            "以逗号分隔的 key=value 列表。",
            "grpc.max_connection_age_ms=2000",
            Text,
            3,
            "网络进阶",
            "all",
            ""
        ),
        field!(
            "allowed_origins",
            "CORS 来源",
            "以逗号分隔；OVMS 默认为 *。",
            "*",
            Text,
            3,
            "访问与媒体",
            "all",
            ""
        ),
        field!(
            "allowed_methods",
            "CORS 方法",
            "以逗号分隔；OVMS 默认为 *。",
            "*",
            Text,
            3,
            "访问与媒体",
            "all",
            ""
        ),
        field!(
            "allowed_headers",
            "CORS Headers",
            "以逗号分隔；OVMS 默认为 *。",
            "*",
            Text,
            3,
            "访问与媒体",
            "all",
            ""
        ),
        field!(
            "allow_credentials",
            "允许 CORS 凭据",
            "对应 --allow_credentials。",
            "",
            Bool,
            3,
            "访问与媒体",
            "all",
            "false"
        ),
        field!(
            "api_key_file",
            "API Key 文件",
            "生成式接口认证密钥文件；界面保存路径，不读取密钥内容。",
            "可选",
            File,
            3,
            "访问与媒体",
            "all",
            ""
        ),
        field!(
            "allowed_media_domains",
            "允许的媒体域名",
            "以逗号分隔；留空禁止使用远程媒体 URL。",
            "例如 example.com",
            Text,
            3,
            "访问与媒体",
            "all",
            ""
        ),
        field!(
            "allowed_local_media_path",
            "本地媒体目录",
            "允许请求引用的本地媒体所在目录。",
            "可选",
            Folder,
            3,
            "访问与媒体",
            "all",
            ""
        ),
        field!(
            "mean",
            "输入均值",
            "预处理减去的值；需要设置 layout。",
            "例如 (123.68,116.78,103.94)",
            Text,
            3,
            "模型预处理",
            "classic",
            ""
        ),
        field!(
            "scale",
            "输入缩放",
            "预处理除以的值；需要设置 layout。",
            "例如 255",
            Text,
            3,
            "模型预处理",
            "classic",
            ""
        ),
        field!(
            "color_format",
            "颜色格式转换",
            "例如 BGR:RGB；需要设置 layout。",
            "继承模型",
            Text,
            3,
            "模型预处理",
            "classic",
            ""
        ),
        field!(
            "precision",
            "输入精度",
            "输入精度覆盖；需要设置 layout。",
            "继承模型",
            Text,
            3,
            "模型预处理",
            "classic",
            ""
        ),
        field!(
            "cpu_extension",
            "CPU 自定义算子库",
            "指定自定义 CPU 层实现的动态库。",
            "可选",
            File,
            3,
            "自定义扩展",
            "all",
            ""
        ),
        field!(
            "custom_node_resources_cleaner_interval_seconds",
            "自定义节点资源清理间隔 / 秒",
            "CLI 默认 300；0 关闭清理。",
            "300",
            Text,
            3,
            "自定义扩展",
            "all",
            ""
        ),
        field!(
            "verbose_response",
            "详细生成响应",
            "响应加入 __verbose 调试信息。",
            "",
            Bool,
            3,
            "自定义扩展",
            "all",
            "false"
        ),
        field!(
            "disable_input_count_validation",
            "忽略请求中的额外输入",
            "关闭额外输入数量检查；必需输入和形状仍会验证。",
            "",
            Bool,
            3,
            "自定义扩展",
            "all",
            "false"
        ),
        field!(
            "draft_model_path",
            "本地 Draft 模型",
            "已导出的 OpenVINO Draft 模型目录；与 draft_source_model 互斥。",
            "可选",
            Folder,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "draft_source_model",
            "Draft 模型来源",
            "Hugging Face 仓库或模型目录；与 draft_model_path 互斥。",
            "可选",
            Text,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "draft_device",
            "Draft 设备",
            "默认跟随主模型设备。",
            "例如 CPU / GPU",
            Text,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "max_prompt_len",
            "NPU 最大 Prompt 长度",
            "NPU 专用参数；需要相应设备和模型支持。",
            "继承模型",
            Text,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "kv_cache_precision",
            "KV Cache 精度",
            "留空使用模型默认值；u8 可降低缓存占用。",
            "继承模型",
            Choice,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "model_distribution_policy",
            "多设备分布策略",
            "Tensor 或 Pipeline 并行，需设备支持。",
            "继承模型",
            Choice,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "reasoning_parser",
            "Reasoning 解析器",
            "留空自动检测；none 显式关闭。",
            "none / qwen3 / gptoss ...",
            Text,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "tool_parser",
            "Tool calls 解析器",
            "留空自动检测；none 显式关闭。",
            "none / hermes3 / llama3 ...",
            Text,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "enable_tool_guided_generation",
            "工具 Schema 约束生成",
            "启用时需要明确设置可用的 tool_parser。",
            "",
            Bool,
            3,
            "文本生成进阶",
            "text_generation",
            "false"
        ),
        field!(
            "cache_interval_multiplier",
            "KV Cache 分配间隔倍数",
            "留空使用运行时默认策略。",
            "自动",
            Text,
            3,
            "文本生成进阶",
            "text_generation",
            ""
        ),
        field!(
            "source_loras",
            "LoRA 适配器",
            "逗号分隔的 alias=source 列表，适用于图像生成。",
            "style=org/repo",
            Text,
            3,
            "图像模型进阶",
            "image_generation",
            ""
        ),
        field!(
            "gguf_filename",
            "GGUF 文件名",
            "HF GGUF 仓库中的量化文件；分片文件指定第一片。",
            "可选",
            Text,
            3,
            "下载进阶",
            "hf",
            ""
        ),
        field!(
            "extra_quantization_params",
            "Optimum 量化参数",
            "仅适用于需要转换的 HF 模型。作为一个完整参数传给 OVMS。",
            "例如 --sym --group-size -1",
            Text,
            3,
            "下载进阶",
            "hf",
            ""
        ),
        field!(
            "environment",
            "子进程环境变量 / JSON",
            "继承系统环境。自定义内容会明文保存在配置中，令牌建议使用系统环境变量。",
            "{\"HF_ENDPOINT\":\"https://huggingface.co\"}",
            Json,
            3,
            "扩展参数",
            "all",
            ""
        ),
        field!(
            "extra_args",
            "其他启动参数 / JSON 数组",
            "用于未来版本的新参数。逐项传给进程；不接受 --pull 等仅执行一次的命令。",
            "[\"--new_option\", \"value\"]",
            Json,
            3,
            "扩展参数",
            "all",
            ""
        ),
    ];
    for item in &mut f {
        item.choices = match item.key {
            "task" => &[
                "",
                "text_generation",
                "embeddings",
                "rerank",
                "image_generation",
                "text2speech",
                "speech2text",
            ],
            "weight-format" => &["", "int4", "int8", "fp16", "fp32"],
            "pipeline_type" => &["", "AUTO", "LM", "LM_CB", "VLM", "VLM_CB"],
            "log_level" => &["TRACE", "DEBUG", "INFO", "WARNING", "ERROR"],
            "pooling" => &["", "CLS", "LAST", "MEAN"],
            "model_type" => &["", "kokoro", "speecht5"],
            "kv_cache_precision" => &["", "u8"],
            "model_distribution_policy" => &["", "TENSOR_PARALLEL", "PIPELINE_PARALLEL"],
            _ => &[],
        };
        item.number_min = match item.key {
            "rest_port"
            | "port"
            | "nireq"
            | "cache_size"
            | "file_system_poll_wait_seconds"
            | "idle_unload_timeout_seconds"
            | "custom_node_resources_cleaner_interval_seconds" => Some(0),
            "rest_workers"
            | "grpc_workers"
            | "grpc_max_threads"
            | "grpc_memory_quota"
            | "max_num_seqs"
            | "max_num_batched_tokens"
            | "num_streams"
            | "max_length"
            | "max_allowed_chunks"
            | "max_num_images_per_prompt"
            | "num_images_per_prompt"
            | "default_num_inference_steps"
            | "max_num_inference_steps"
            | "max_prompt_len"
            | "cache_interval_multiplier" => Some(1),
            _ => None,
        };
    }
    f
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub schema_version: u32,
    pub name: String,
    pub mode: Mode,
    pub values: BTreeMap<String, String>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            name: "我的模型服务".into(),
            mode: Mode::Local,
            values: fields()
                .into_iter()
                .map(|f| (f.key.into(), f.default.into()))
                .collect(),
        }
    }
}

impl Profile {
    pub fn value(&self, key: &str) -> &str {
        self.values.get(key).map(|s| s.trim()).unwrap_or("")
    }
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        self.values.insert(key.to_owned(), value.into());
    }
    pub fn applies(&self, field: &Field) -> bool {
        match field.scope {
            "all" => true,
            "local" => self.mode == Mode::Local,
            "hf" => self.mode == Mode::HuggingFace,
            "config" => self.mode == Mode::Config,
            "single" => self.mode != Mode::Config,
            "classic" => self.mode == Mode::Local && self.value("task").is_empty(),
            "streams" => {
                self.mode != Mode::Config
                    && !self.value("task").is_empty()
                    && self.value("task") != "text_generation"
            }
            task => self.mode != Mode::Config && self.value("task") == task,
        }
    }
    pub fn upgrade(&mut self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "不支持配置版本 {}，当前支持版本 1",
                self.schema_version
            ));
        }
        // Runtime selection is an application setting, never an executable in a model profile.
        self.values.remove("executable");
        for f in fields() {
            self.values
                .entry(f.key.into())
                .or_insert_with(|| f.default.into());
        }
        Ok(())
    }
    pub fn arguments(&self) -> Result<Vec<String>, String> {
        self.validate_values()?;
        let mut args = Vec::new();
        for f in fields().iter().filter(|f| self.applies(f)) {
            if matches!(f.key, "working_dir" | "environment" | "extra_args") {
                continue;
            }
            let value = self.value(f.key).trim();
            if value.is_empty() || (f.key == "batch_size" && !self.value("shape").trim().is_empty())
            {
                continue;
            }
            // cxxopts bool options must use =false to avoid implicit-value ambiguity.
            args.push(format!("--{}={value}", f.key));
        }
        args.extend(self.extra_arguments()?);
        Ok(args)
    }
    pub fn validate_values(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("配置文件版本不受支持".into());
        }
        let required: &[&str] = match self.mode {
            Mode::Local => &["model_name", "model_path"],
            Mode::HuggingFace => &["model_name", "source_model", "model_repository_path"],
            Mode::Config => &["config_path"],
        };
        let definitions = fields();
        for key in required {
            if self.value(key).trim().is_empty() {
                let label = definitions.iter().find(|f| f.key == *key).unwrap().label;
                return Err(format!("请填写或选择{label}"));
            }
        }
        for f in definitions.iter().filter(|f| self.applies(f)) {
            let value = self.value(f.key).trim();
            if value.is_empty() {
                continue;
            }
            if value.contains('\0') {
                return Err(format!("{}不能包含空字符", f.label));
            }
            if let Some(min) = f.number_min {
                let n: u64 = value
                    .parse()
                    .map_err(|_| format!("{}需要非负整数", f.label))?;
                let max = if matches!(f.key, "rest_port" | "port") {
                    65535
                } else if f.key == "grpc_memory_quota" || f.key == "cache_interval_multiplier" {
                    u64::MAX
                } else {
                    u32::MAX as u64
                };
                if n < min || n > max {
                    return Err(format!("{}应在 {min}–{max} 之间", f.label));
                }
            }
            if f.kind == Kind::Bool && !["true", "false"].contains(&value) {
                return Err(format!("{}需要 true 或 false", f.label));
            }
            if !f.choices.is_empty() && !f.choices.contains(&value) {
                return Err(format!("{}不支持值 {value}", f.label));
            }
        }
        let rest = self.port("rest_port")?;
        let grpc = self.port("port")?;
        if rest == 0 && grpc == 0 {
            return Err("REST 与 gRPC 至少启用一个端口".into());
        }
        if rest != 0 && rest == grpc {
            return Err("REST 与 gRPC 端口不能相同".into());
        }
        if rest == 0
            && (self.value("metrics_enable") == "true"
                || !self.value("metrics_list").trim().is_empty())
        {
            return Err("Prometheus 指标需要启用 REST 端口".into());
        }
        if !self.value("metrics_list").trim().is_empty() && self.value("metrics_enable") != "true" {
            return Err("填写指标列表时请同时启用 Prometheus 指标".into());
        }
        if self.mode != Mode::Config {
            let name = self.value("model_name").trim();
            if name.chars().any(|c| c.is_control()) {
                return Err("模型名称不能包含控制字符".into());
            }
            if !self.value("plugin_config").trim().is_empty() {
                json_object(self.value("plugin_config"), "插件配置")?;
            }
            if self.mode == Mode::HuggingFace {
                let source = self.value("source_model").trim();
                if !source.contains('/') || source.contains(char::is_whitespace) {
                    return Err("HF 仓库应为组织/模型名称，例如 OpenVINO/Qwen3-0.6B-int4-ov".into());
                }
                if source.starts_with("OpenVINO/")
                    && (!self.value("weight-format").is_empty()
                        || !self.value("extra_quantization_params").is_empty())
                {
                    return Err("OpenVINO 已导出模型不能设置 weight-format 或额外量化参数".into());
                }
            }
            if self.value("task").is_empty() && self.mode == Mode::Local {
                let batch = self.value("batch_size").trim();
                if self.value("shape").trim().is_empty()
                    && !batch.is_empty()
                    && batch != "auto"
                    && batch.parse::<u32>().ok().filter(|n| *n > 0).is_none()
                {
                    return Err("批大小应为正整数或 auto".into());
                }
                for key in ["mean", "scale", "color_format", "precision"] {
                    if !self.value(key).trim().is_empty() && self.value("layout").trim().is_empty()
                    {
                        return Err(format!("{key} 需要同时设置 layout"));
                    }
                }
                if !self.value("model_version_policy").trim().is_empty() {
                    validate_version_policy(self.value("model_version_policy"))?;
                }
            }
            if self.value("task") == "text_generation" {
                if !self.value("draft_model_path").is_empty()
                    && !self.value("draft_source_model").is_empty()
                {
                    return Err("本地 Draft 模型与 Draft 来源不能同时设置".into());
                }
                if self.value("enable_tool_guided_generation") == "true"
                    && (self.value("tool_parser").is_empty() || self.value("tool_parser") == "none")
                {
                    return Err("工具 Schema 约束生成需要设置 tool_parser".into());
                }
            }
            if self.value("task") == "image_generation" {
                self.validate_image_options()?;
            }
        }
        self.environment()?;
        self.extra_arguments()?;
        Ok(())
    }
    fn validate_image_options(&self) -> Result<(), String> {
        let resolution = self.value("resolution").trim();
        for key in ["resolution", "max_resolution", "default_resolution"] {
            let value = self.value(key).trim();
            if value.is_empty() {
                continue;
            }
            for size in value.split_whitespace() {
                let valid = size
                    .split_once('x')
                    .map(|(w, h)| {
                        w.parse::<u32>().ok().filter(|n| *n > 0).is_some()
                            && h.parse::<u32>().ok().filter(|n| *n > 0).is_some()
                    })
                    .unwrap_or(false);
                if !valid {
                    return Err(format!("{key} 需要 WxH 格式，例如 512x512"));
                }
            }
            if key != "resolution" && value.split_whitespace().count() != 1 {
                return Err(format!("{key} 只能指定一个分辨率"));
            }
        }
        if (!self.value("num_images_per_prompt").is_empty()
            || !self.value("guidance_scale").is_empty())
            && resolution.split_whitespace().count() != 1
        {
            return Err("静态管线图片数和 Guidance scale 需要且只能设置一个 resolution".into());
        }
        if !self.value("guidance_scale").is_empty()
            && self
                .value("guidance_scale")
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .is_none()
        {
            return Err("Guidance scale 需要非负有限数值".into());
        }
        Ok(())
    }
    pub fn port(&self, key: &str) -> Result<u16, String> {
        let value = self.value(key).trim();
        if value.is_empty() {
            return Ok(0);
        }
        value
            .parse()
            .map_err(|_| format!("{key} 需要 0–65535 之间的端口"))
    }
    pub fn extra_arguments(&self) -> Result<Vec<String>, String> {
        let raw = self.value("extra_args").trim();
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        let args: Vec<String> =
            serde_json::from_str(raw).map_err(|e| format!("其他参数需要 JSON 字符串数组：{e}"))?;
        let known: BTreeSet<_> = fields().into_iter().map(|f| f.key).collect();
        for arg in &args {
            if arg.contains('\0') {
                return Err("扩展参数不能包含空字符".into());
            }
            let flag = arg.split('=').next().unwrap_or(arg).trim_start_matches('-');
            if [
                "pull",
                "configure",
                "help",
                "h",
                "version",
                "list_models",
                "add_to_config",
                "remove_from_config",
                "overwrite_models",
            ]
            .contains(&flag)
            {
                return Err(format!(
                    "--{flag} 不属于启动服务所需的参数，请单独使用 OVMS CLI 执行"
                ));
            }
            if arg.starts_with('-') && known.contains(flag) {
                return Err(format!("--{flag} 已有专用输入项，请在那里设置"));
            }
            if !arg.starts_with('-') && flag.is_empty() {
                return Err("扩展参数不能包含空字符串".into());
            }
        }
        if args.first().is_some_and(|s| !s.starts_with("--")) {
            return Err("扩展参数数组需要从 --参数名 开始".into());
        }
        Ok(args)
    }
    pub fn environment(&self) -> Result<BTreeMap<String, String>, String> {
        let raw = self.value("environment").trim();
        if raw.is_empty() {
            return Ok(BTreeMap::new());
        }
        let env: BTreeMap<String, String> =
            serde_json::from_str(raw).map_err(|e| format!("环境变量需要 JSON 字符串对象：{e}"))?;
        for (key, value) in &env {
            if key.is_empty() || key.contains(['=', '\0']) || value.contains('\0') {
                return Err("环境变量名称或内容无效".into());
            }
        }
        Ok(env)
    }
    pub fn endpoint(&self) -> String {
        self.port("rest_port")
            .ok()
            .filter(|p| *p != 0)
            .map(|port| {
                format!(
                    "http://{}:{port}",
                    url_host(self.value("rest_bind_address"))
                )
            })
            .unwrap_or_else(|| "REST 已关闭".into())
    }
    pub fn grpc_endpoint(&self) -> String {
        self.port("port")
            .ok()
            .filter(|p| *p != 0)
            .map(|port| format!("{}:{port}", url_host(self.value("grpc_bind_address"))))
            .unwrap_or_else(|| "gRPC 已关闭".into())
    }
    pub fn preview(&self) -> String {
        match self.arguments() {
            Ok(args) => display_command("ovms", &args),
            Err(e) => format!("填写完整配置后生成启动命令\n{e}"),
        }
    }
    pub fn absolutize_paths(&mut self, base: &Path) {
        for field in fields() {
            if matches!(field.kind, Kind::File | Kind::Folder | Kind::Model)
                || field.key == "log_path"
            {
                let value = self.value(field.key).to_owned();
                if value.is_empty() || is_cloud_path(&value) {
                    continue;
                }
                if Path::new(&value).is_relative() {
                    self.set(field.key, base.join(value).to_string_lossy());
                }
            }
        }
    }
}

fn json_object(raw: &str, label: &str) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("{label}不是合法 JSON：{e}"))?;
    if !value.is_object() {
        return Err(format!("{label}需要 JSON 对象"));
    }
    Ok(value)
}

fn validate_version_policy(raw: &str) -> Result<(), String> {
    let v = json_object(raw, "版本策略")?;
    let obj = v.as_object().unwrap();
    let valid = if obj.len() != 1 {
        false
    } else if let Some(latest) = obj.get("latest") {
        latest
            .get("num_versions")
            .and_then(|n| n.as_u64())
            .is_some_and(|n| n > 0)
    } else if let Some(all) = obj.get("all") {
        all.as_object().is_some_and(|o| o.is_empty())
    } else if let Some(specific) = obj.get("specific") {
        specific
            .get("versions")
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty() && a.iter().all(|v| v.as_u64().is_some_and(|n| n > 0)))
    } else {
        false
    };
    if valid {
        Ok(())
    } else {
        Err("版本策略需要 latest.num_versions、all:{} 或 specific.versions".into())
    }
}

pub fn is_cloud_path(s: &str) -> bool {
    ["s3://", "gs://", "azure://", "az://"]
        .iter()
        .any(|p| s.starts_with(p))
}

pub fn connect_host(bind: &str) -> &str {
    let host = bind
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches(['[', ']']);
    match host {
        "" | "0.0.0.0" | "*" => "127.0.0.1",
        "::" => "::1",
        _ => host,
    }
}
fn url_host(bind: &str) -> String {
    let host = connect_host(bind);
    if host.contains(':') {
        format!("[{host}]")
    } else {
        host.into()
    }
}

pub fn display_command(executable: &str, args: &[String]) -> String {
    std::iter::once(executable)
        .chain(args.iter().map(String::as_str))
        .map(quote_argument)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_argument(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_alphanumeric() || "-_=./:\\,".contains(c))
    {
        return arg.into();
    }
    if cfg!(windows) {
        let mut out = String::from("\"");
        let mut slashes = 0;
        for c in arg.chars() {
            if c == '\\' {
                slashes += 1;
                continue;
            }
            if c == '"' {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
            } else {
                out.extend(std::iter::repeat_n('\\', slashes));
            }
            slashes = 0;
            out.push(c);
        }
        out.extend(std::iter::repeat_n('\\', slashes * 2));
        out.push('"');
        out
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Library {
    pub current: Profile,
    pub saved: BTreeMap<String, Profile>,
    pub runtime: RuntimeSettings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    #[default]
    Native,
    Docker,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeSettings {
    pub backend: Backend,
    pub docker_acceleration: bool,
}

pub fn data_directory() -> PathBuf {
    // This override keeps integration tests and portable deployments isolated.
    std::env::var_os("OVMSTUDIO_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            directories::ProjectDirs::from("dev", "OVMSStudio", "OVMS Studio")
                .map(|d| d.data_local_dir().join("runtimes"))
                .unwrap_or_else(|| PathBuf::from("ovms-studio-runtimes"))
        })
}

pub fn state_path() -> PathBuf {
    directories::ProjectDirs::from("dev", "OVMSStudio", "OVMS Studio")
        .map(|d| d.config_dir().join("profiles.json"))
        .unwrap_or_else(|| PathBuf::from("ovms-studio-profiles.json"))
}

pub fn save_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &data).map_err(|e| format!("无法写入配置：{e}"))?;
    // rename replaces the destination atomically on both Windows and Unix.
    std::fs::rename(&tmp, path).map_err(|e| format!("无法保存配置：{e}"))
}
