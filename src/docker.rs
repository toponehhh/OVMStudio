//! Docker CLI integration. Arguments are passed directly, never through a shell.
use crate::config::{self, Mode, Profile};
use crate::deployment::{Cancellation, PreparedRuntime, new_cancellation, unique_id};
use crate::runtime::{self, LaunchSpec};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Container {
    pub program: PathBuf,
    pub name: String,
}

impl Container {
    pub fn remove(&self) -> Result<(), String> {
        match output(
            &self.program,
            &[
                "container".into(),
                "rm".into(),
                "--force".into(),
                self.name.clone(),
            ],
            Duration::from_secs(10),
        ) {
            Ok(_) => Ok(()),
            Err(error) if error.to_lowercase().contains("no such container") => Ok(()),
            Err(error) => Err(format!("清理容器 {} 失败：{error}", self.name)),
        }
    }
}

fn output(program: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    runtime::run_output(
        &mut runtime::command_for(program, &cwd, &BTreeMap::new(), args),
        timeout,
        &new_cancellation(),
    )
}

pub fn find_program() -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        "docker.exe"
    } else {
        "docker"
    };
    let mut candidates = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .collect::<Vec<_>>();
    if cfg!(windows)
        && let Some(program_files) = std::env::var_os("ProgramFiles")
    {
        candidates
            .push(PathBuf::from(program_files).join("Docker/Docker/resources/bin/docker.exe"));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
        .ok_or("未安装 Docker。请先安装 Docker Desktop 并启动 Linux 容器引擎，然后重试。".into())
}

pub fn detect() -> Result<PathBuf, String> {
    let program = find_program()?;
    output(&program, &["--version".into()], Duration::from_secs(8))
        .map_err(|e| format!("无法运行 Docker 客户端：{e}"))?;
    if std::env::var("DOCKER_HOST").is_ok_and(|v| v.starts_with("tcp:") || v.starts_with("ssh:")) {
        return Err(
            "当前使用远程 Docker；本机模型自动挂载需要本机 Docker 引擎，请切换 Docker context"
                .into(),
        );
    }
    let host = output(
        &program,
        &[
            "context".into(),
            "inspect".into(),
            "--format".into(),
            "{{.Endpoints.docker.Host}}".into(),
        ],
        Duration::from_secs(8),
    )?;
    if host.trim().starts_with("tcp:") || host.trim().starts_with("ssh:") {
        return Err("模型自动挂载仅支持本机 Docker context，请切换到本机引擎".into());
    }
    let os = output(
        &program,
        &["info".into(), "--format".into(), "{{.OSType}}".into()],
        Duration::from_secs(10),
    )
    .map_err(|e| format!("Docker 客户端已安装，但无法连接引擎。请启动 Docker Desktop：{e}"))?;
    if !os.lines().any(|line| line.trim() == "linux") {
        return Err(
            "OVMS 镜像需要 Linux 容器，请在 Docker Desktop 中切换到 Linux containers".into(),
        );
    }
    Ok(program)
}

pub fn valid_image_id(image: &str) -> bool {
    image
        .strip_prefix("sha256:")
        .is_some_and(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn image_exists(program: &Path, image: &str) -> bool {
    valid_image_id(image)
        && output(
            program,
            &[
                "image".into(),
                "inspect".into(),
                "--format".into(),
                "{{.Id}}".into(),
                image.into(),
            ],
            Duration::from_secs(10),
        )
        .is_ok()
}

pub fn pull_image(
    program: &Path,
    image: &str,
    cancel: &Cancellation,
    progress: &dyn Fn(String),
) -> Result<String, String> {
    progress(format!("正在拉取 {image}，首次下载可能需要几分钟…"));
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    runtime::run_output(
        &mut runtime::command_for(
            program,
            &cwd,
            &BTreeMap::new(),
            &["pull".into(), "--quiet".into(), image.into()],
        ),
        Duration::from_secs(1200),
        cancel,
    )
    .map_err(|e| format!("Docker 镜像下载失败：{e}"))?;
    let inspected = output(
        program,
        &[
            "image".into(),
            "inspect".into(),
            "--format".into(),
            "{{.Id}}".into(),
            image.into(),
        ],
        Duration::from_secs(10),
    )?;
    let id = inspected
        .lines()
        .map(str::trim)
        .find(|line| valid_image_id(line))
        .ok_or("Docker 返回的镜像 ID 无效")?
        .to_owned();
    progress(format!("镜像已准备：{image}"));
    Ok(id)
}

#[derive(Clone, Debug)]
struct Mount {
    host: PathBuf,
    target: String,
    writable: bool,
}

struct Paths {
    cwd: PathBuf,
    mounts: Vec<Mount>,
}

impl Paths {
    fn map(&mut self, raw: &str, writable: bool, directory: bool) -> Result<String, String> {
        let path = Path::new(raw);
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            self.cwd.join(path)
        };
        if writable {
            let parent = if directory {
                path.as_path()
            } else {
                path.parent().ok_or("输出路径缺少父目录")?
            };
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建输出目录 {}：{e}", parent.display()))?;
        }
        let is_dir = path.is_dir();
        if !writable && !path.exists() {
            return Err(format!("路径不存在：{}", path.display()));
        }
        let parent = if is_dir {
            path.as_path()
        } else {
            path.parent().ok_or("模型路径缺少父目录")?
        };
        let host = std::fs::canonicalize(parent)
            .map_err(|e| format!("模型或配置目录不存在 {}：{e}", parent.display()))?;
        let resolved = if is_dir {
            host.clone()
        } else {
            host.join(path.file_name().ok_or("路径缺少文件名")?)
        };
        if !writable && resolved.starts_with(&self.cwd) {
            let relative = resolved
                .strip_prefix(&self.cwd)
                .map_err(|e| e.to_string())?;
            return Ok(format!(
                "/workspace/{}",
                relative.to_string_lossy().replace('\\', "/")
            ));
        }
        let mount = if let Some(index) = self
            .mounts
            .iter()
            .position(|m| m.host == host && m.writable == writable)
        {
            index
        } else {
            let target = format!("/ovmstudio/data/{}", self.mounts.len());
            self.mounts.push(Mount {
                host,
                target,
                writable,
            });
            self.mounts.len() - 1
        };
        Ok(if is_dir {
            self.mounts[mount].target.clone()
        } else {
            format!(
                "{}/{}",
                self.mounts[mount].target,
                path.file_name().ok_or("路径缺少文件名")?.to_string_lossy()
            )
        })
    }
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.into()
    }
}

pub fn mount_source(path: &Path) -> String {
    let raw = path.to_string_lossy();
    if cfg!(windows) {
        if let Some(path) = raw.strip_prefix("\\\\?\\UNC\\") {
            return format!("//{}", path.replace('\\', "/"));
        }
        raw.strip_prefix("\\\\?\\")
            .unwrap_or(&raw)
            .replace('\\', "/")
    } else {
        raw.into_owned()
    }
}

fn rewrite_config(value: &mut serde_json::Value, paths: &mut Paths) -> Result<(), String> {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                if matches!(
                    key.as_str(),
                    "base_path" | "model_path" | "graph_path" | "tokenizer_path" | "subconfig"
                ) && let Some(raw) = value
                    .as_str()
                    .filter(|s| !s.is_empty() && !config::is_cloud_path(s))
                {
                    *value = serde_json::Value::String(paths.map(raw, false, false)?);
                } else {
                    rewrite_config(value, paths)?;
                }
            }
        }
        serde_json::Value::Array(array) => {
            for value in array {
                rewrite_config(value, paths)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn requested_devices(
    profile: &Profile,
    application: &Path,
) -> Result<std::collections::BTreeSet<&'static str>, String> {
    let mut devices = std::collections::BTreeSet::new();
    fn collect(value: &serde_json::Value, devices: &mut std::collections::BTreeSet<&'static str>) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    if key == "target_device"
                        && let Some(device) = value.as_str()
                    {
                        if device.contains("GPU") {
                            devices.insert("GPU");
                        }
                        if device.contains("NPU") {
                            devices.insert("NPU");
                        }
                    } else {
                        collect(value, devices);
                    }
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    collect(value, devices);
                }
            }
            _ => {}
        }
    }
    if profile.mode == Mode::Config && !profile.value("config_path").is_empty() {
        let cwd = runtime::working_directory(application, profile.value("working_dir"))?;
        let raw = Path::new(profile.value("config_path"));
        let path = if raw.is_absolute() {
            raw.to_owned()
        } else {
            cwd.join(raw)
        };
        let json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path).map_err(|e| format!("无法读取配置：{e}"))?,
        )
        .map_err(|e| e.to_string())?;
        collect(&json, &mut devices);
    } else {
        let mut keys = vec!["target_device"];
        if profile.mode != Mode::Config && profile.value("task") == "text_generation" {
            keys.push("draft_device");
        }
        for key in keys {
            if profile.value(key).contains("GPU") {
                devices.insert("GPU");
            }
            if profile.value(key).contains("NPU") {
                devices.insert("NPU");
            }
        }
    }
    Ok(devices)
}

pub fn launch(
    profile: &Profile,
    ready: &PreparedRuntime,
    application: &Path,
    data: &Path,
) -> Result<LaunchSpec, String> {
    profile.validate_values()?;
    let program = ready.docker_program.clone().ok_or("Docker 尚未检查")?;
    let image = ready
        .docker_image_id
        .as_deref()
        .filter(|id| valid_image_id(id))
        .ok_or("Docker 镜像尚未下载")?;
    let cwd = runtime::working_directory(application, profile.value("working_dir"))?;
    let devices = requested_devices(profile, application)?;
    if !devices.is_empty() && !ready.docker_acceleration {
        return Err("GPU/NPU 需要加速器镜像，请重新准备运行环境".into());
    }
    if cfg!(windows) && devices.contains("NPU") {
        return Err(
            "Docker NPU 需要 Linux 宿主驱动及 /dev/accel；Windows NPU 请使用本机模式".into(),
        );
    }
    let mut paths = Paths {
        cwd: cwd.clone(),
        mounts: vec![Mount {
            host: cwd.clone(),
            target: "/workspace".into(),
            writable: false,
        }],
    };
    let mut translated = profile.clone();
    if profile.value("cache_dir").is_empty() {
        translated.set(
            "cache_dir",
            paths.map(&data.join("docker-cache").to_string_lossy(), true, true)?,
        );
    }
    for field in config::fields().iter().filter(|f| profile.applies(f)) {
        let raw = profile.value(field.key);
        if raw.is_empty()
            || config::is_cloud_path(raw)
            || matches!(field.key, "working_dir" | "config_path")
        {
            continue;
        }
        if matches!(
            field.kind,
            config::Kind::File | config::Kind::Folder | config::Kind::Model
        ) || field.key == "log_path"
        {
            let writable = matches!(
                field.key,
                "cache_dir" | "model_repository_path" | "log_path"
            );
            if !writable && !Path::new(raw).is_absolute() && !cwd.join(raw).exists() {
                return Err(format!("{} 不存在：{raw}", field.label));
            }
            if !writable && Path::new(raw).is_absolute() && !Path::new(raw).exists() {
                return Err(format!("{} 不存在：{raw}", field.label));
            }
            if field.key == "cpu_extension"
                && Path::new(raw)
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("dll"))
            {
                return Err(
                    "Docker 内运行 Linux OVMS，CPU 自定义算子必须使用 Linux .so 文件".into(),
                );
            }
            translated.set(
                field.key,
                paths.map(raw, writable, matches!(field.kind, config::Kind::Folder))?,
            );
        }
    }
    for key in ["source_model", "draft_source_model"] {
        let raw = profile.value(key);
        if !raw.is_empty() && (Path::new(raw).is_absolute() || cwd.join(raw).exists()) {
            translated.set(key, paths.map(raw, false, false)?);
        }
    }
    if config::fields()
        .iter()
        .any(|field| field.key == "source_loras" && profile.applies(field))
        && !profile.value("source_loras").is_empty()
    {
        let mut mapped = Vec::new();
        for entry in profile.value("source_loras").split(',') {
            let (alias, source) = entry
                .split_once('=')
                .ok_or("LoRA 来源需要 alias=source 格式")?;
            let source = if Path::new(source).is_absolute() || cwd.join(source).exists() {
                paths.map(source, false, false)?
            } else {
                source.to_owned()
            };
            mapped.push(format!("{alias}={source}"));
        }
        translated.set("source_loras", mapped.join(","));
    }
    if profile.mode == Mode::Config {
        let original = Path::new(profile.value("config_path"));
        let original = if original.is_absolute() {
            original.to_owned()
        } else {
            cwd.join(original)
        };
        let mut json: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&original).map_err(|e| format!("无法读取 config.json：{e}"))?,
        )
        .map_err(|e| e.to_string())?;
        if !json.is_object() {
            return Err("config.json 顶层需要 JSON 对象".into());
        }
        rewrite_config(&mut json, &mut paths)?;
        let generated = data
            .join("container-configs")
            .join(unique_id())
            .join("config.json");
        config::save_json(&generated, &json)?;
        translated.set(
            "config_path",
            paths.map(&generated.to_string_lossy(), false, false)?,
        );
    }
    let container = Container {
        program: program.clone(),
        name: format!("ovmstudio-{}", unique_id()),
    };
    let mut args = vec![
        "run".into(),
        "--rm".into(),
        "--init".into(),
        "--pull=never".into(),
        "--name".into(),
        container.name.clone(),
        "--label".into(),
        "dev.ovmstudio.managed=true".into(),
        "--workdir".into(),
        "/workspace".into(),
    ];
    if devices.contains("GPU") {
        if cfg!(windows) || Path::new("/dev/dxg").exists() {
            args.extend([
                "--device".into(),
                "/dev/dxg".into(),
                "--volume".into(),
                "/usr/lib/wsl:/usr/lib/wsl:ro".into(),
            ]);
        } else {
            args.extend(["--device".into(), "/dev/dri".into()]);
        }
    }
    if devices.contains("NPU") {
        args.extend(["--device".into(), "/dev/accel".into()]);
    }
    #[cfg(unix)]
    {
        // Query the caller identity so writes to bind-mounted caches keep host ownership.
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };
        args.extend(["--user".into(), format!("{uid}:{gid}")]);
        use std::os::unix::fs::MetadataExt;
        let mut groups = std::collections::BTreeSet::new();
        for (device, directory) in [("GPU", "/dev/dri"), ("NPU", "/dev/accel")] {
            if devices.contains(device)
                && let Ok(entries) = std::fs::read_dir(directory)
            {
                for entry in entries.flatten() {
                    if let Ok(metadata) = entry.metadata() {
                        groups.insert(metadata.gid());
                    }
                }
            }
        }
        for group in groups {
            args.extend(["--group-add".into(), group.to_string()]);
        }
    }
    #[cfg(windows)]
    args.extend(["--user".into(), "0".into()]);
    args.extend([
        "--env".into(),
        "HOME=/tmp".into(),
        "--env".into(),
        format!("HF_HOME={}/huggingface", translated.value("cache_dir")),
        "--env".into(),
        format!("XDG_CACHE_HOME={}", translated.value("cache_dir")),
    ]);
    for (port_key, bind_key) in [
        ("rest_port", "rest_bind_address"),
        ("port", "grpc_bind_address"),
    ] {
        let port = profile.port(port_key)?;
        if port == 0 {
            continue;
        }
        let bind = profile.value(bind_key);
        for host in (if bind.is_empty() { "0.0.0.0" } else { bind }).split(',') {
            let host = host.trim().trim_matches(['[', ']']);
            let host = if host.contains(':') {
                format!("[{host}]")
            } else {
                host.to_owned()
            };
            args.extend(["--publish".into(), format!("{host}:{port}:{port}")]);
        }
        translated.set(bind_key, "0.0.0.0");
    }
    for mount in paths.mounts {
        let source = csv_field(&format!("source={}", mount_source(&mount.host)));
        args.extend([
            "--mount".into(),
            format!(
                "type=bind,{source},target={}{}",
                mount.target,
                if mount.writable { "" } else { ",readonly" }
            ),
        ]);
    }
    let custom = profile.environment()?;
    if custom
        .keys()
        .any(|key| key.to_uppercase().starts_with("DOCKER_"))
    {
        return Err("Docker 连接选项请通过系统环境设置，模型环境变量中不能包含 DOCKER_*".into());
    }
    let mut env: BTreeMap<OsString, OsString> =
        custom.iter().map(|(k, v)| (k.into(), v.into())).collect();
    // A container HOME override must not switch the client's Docker context.
    if let Some(directory) = std::env::var_os("DOCKER_CONFIG")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".docker")))
    {
        env.insert("DOCKER_CONFIG".into(), directory.into_os_string());
    }
    let mut keys = custom
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    for key in ["HF_TOKEN", "HUGGING_FACE_HUB_TOKEN", "HF_ENDPOINT"] {
        if let Some(value) = std::env::var_os(key) {
            env.entry(key.into()).or_insert(value);
            keys.insert(key.into());
        }
    }
    for key in keys {
        args.extend(["--env".into(), key]);
    }
    args.push(image.into());
    args.extend(translated.arguments()?);
    Ok(LaunchSpec {
        program,
        args,
        cwd,
        env,
        profile: profile.clone(),
        container: Some(container),
    })
}

pub fn inspect_runtime(
    profile: &Profile,
    ready: &PreparedRuntime,
) -> Result<(String, String), String> {
    let program = ready.docker_program.as_deref().ok_or("Docker 尚未检查")?;
    let image = ready
        .docker_image_id
        .as_deref()
        .filter(|id| valid_image_id(id))
        .ok_or("Docker 镜像尚未准备")?;
    let run = |extra: Vec<String>| -> Result<String, String> {
        let container = Container {
            program: program.to_owned(),
            name: format!("ovmstudio-inspect-{}", unique_id()),
        };
        let mut args = vec![
            "run".into(),
            "--rm".into(),
            "--name".into(),
            container.name.clone(),
            "--network=none".into(),
            "--pull=never".into(),
            image.into(),
        ];
        args.extend(extra);
        let result = output(program, &args, Duration::from_secs(30));
        let _ = container.remove();
        result
    };
    let version = run(vec!["--version".into()])?;
    let mut args = vec!["--help".into()];
    if profile.mode != Mode::Config && !profile.value("task").is_empty() {
        args.push(format!("--task={}", profile.value("task")));
    }
    Ok((version.trim().into(), run(args)?))
}
