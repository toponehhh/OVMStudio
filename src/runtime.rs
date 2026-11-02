use crate::config::{Mode, Profile, bundled_program_relative, connect_host, is_cloud_path};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
    pub profile: Profile,
}

pub fn bundle_directory() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = executable.parent().ok_or("无法确定应用目录")?.to_owned();
    #[cfg(debug_assertions)]
    if !directory.join(bundled_program_relative()).is_file() {
        // This development-only location is never compiled into the release launcher.
        let development = Path::new(env!("CARGO_MANIFEST_DIR")).join("work/runtime-python-on");
        if development.join(bundled_program_relative()).is_file() {
            return Ok(development);
        }
    }
    Ok(directory)
}

pub fn resolve_bundled_program(bundle: &Path) -> Result<PathBuf, String> {
    let program = bundle.join(bundled_program_relative());
    if !program.is_file() {
        return Err(
            "应用内置 OVMS 缺失，请重新解压完整的 OVMS Studio 程序包，并保留 ovms 子目录。".into(),
        );
    }
    std::fs::canonicalize(program).map_err(|e| format!("无法访问内置 OVMS：{e}"))
}

impl LaunchSpec {
    pub fn prepare(profile: &Profile) -> Result<Self, String> {
        Self::prepare_in_bundle(profile, &bundle_directory()?)
    }

    pub fn prepare_in_bundle(profile: &Profile, bundle: &Path) -> Result<Self, String> {
        profile.validate_values()?;
        let program = resolve_bundled_program(bundle)?;
        let cwd = working_directory(bundle, profile.value("working_dir"))?;
        let local_path = |s: &str| {
            let p = Path::new(s);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        };
        match profile.mode {
            Mode::Local => {
                let p = profile.value("model_path").trim();
                if !is_cloud_path(p) && !local_path(p).exists() {
                    return Err(format!("模型路径不存在：{}", local_path(p).display()));
                }
            }
            Mode::Config => {
                let p = local_path(profile.value("config_path").trim());
                let data = std::fs::read_to_string(&p)
                    .map_err(|e| format!("无法读取配置 {}：{e}", p.display()))?;
                let json: serde_json::Value = serde_json::from_str(&data)
                    .map_err(|e| format!("config.json 不是合法 JSON：{e}"))?;
                if !json.is_object() {
                    return Err("config.json 顶层需要 JSON 对象".into());
                }
            }
            Mode::HuggingFace => {
                let p = local_path(profile.value("model_repository_path").trim());
                if p.exists() && !p.is_dir() {
                    return Err("模型下载目录是一个文件，请选择文件夹".into());
                }
            }
        }
        for key in ["api_key_file", "cpu_extension"] {
            if !profile.value(key).trim().is_empty() && !local_path(profile.value(key)).is_file() {
                return Err(format!("{key} 文件不存在"));
            }
        }
        if profile.mode != Mode::Config
            && profile.value("task") == "text_generation"
            && !profile.value("draft_model_path").is_empty()
            && !local_path(profile.value("draft_model_path")).is_dir()
        {
            return Err("本地 Draft 模型目录不存在".into());
        }
        let env = runtime_environment(&program, &profile.environment()?);
        Ok(Self {
            program,
            args: profile.arguments()?,
            cwd,
            env,
            profile: profile.clone(),
        })
    }

    pub fn command(&self) -> Command {
        command_for(&self.program, &self.cwd, &self.env, &self.args)
    }

    pub fn check_ports(&self) -> Result<(), String> {
        // Keep all listeners alive until every configured interface has been checked.
        let mut listeners = Vec::new();
        for (port_key, bind_key) in [
            ("rest_port", "rest_bind_address"),
            ("port", "grpc_bind_address"),
        ] {
            let port = self.profile.port(port_key)?;
            if port == 0 {
                continue;
            }
            let bind = self.profile.value(bind_key).trim();
            let bind = if bind.is_empty() { "0.0.0.0" } else { bind };
            for host in bind.split(',').map(|h| h.trim().trim_matches(['[', ']'])) {
                if host.is_empty() {
                    return Err(format!("{bind_key} 中有空地址"));
                }
                let listener = TcpListener::bind((host, port)).map_err(|e| {
                    format!("无法监听 {host}:{port}：{e}。请检查端口占用和监听地址。")
                })?;
                listeners.push(listener);
            }
        }
        Ok(())
    }

    pub fn readiness(&self) -> Result<bool, String> {
        let port = self.profile.port("rest_port")?;
        if port == 0 {
            return Err("仅启用 gRPC：进程正在运行，未验证模型就绪".into());
        }
        http_ready(connect_host(self.profile.value("rest_bind_address")), port)
    }
}

fn working_directory(bundle: &Path, raw: &str) -> Result<PathBuf, String> {
    let p = if raw.trim().is_empty() {
        bundle.to_owned()
    } else if Path::new(raw.trim()).is_relative() {
        bundle.join(raw.trim())
    } else {
        PathBuf::from(raw.trim())
    };
    if !p.is_dir() {
        return Err(format!("工作目录不存在：{}", p.display()));
    }
    std::fs::canonicalize(p).map_err(|e| e.to_string())
}

fn runtime_environment(
    program: &Path,
    custom: &BTreeMap<String, String>,
) -> BTreeMap<OsString, OsString> {
    let mut env: BTreeMap<OsString, OsString> =
        custom.iter().map(|(k, v)| (k.into(), v.into())).collect();
    let dir = program.parent().unwrap_or(Path::new("."));
    let root = if dir.file_name().is_some_and(|n| n == "bin") {
        dir.parent().unwrap_or(dir)
    } else {
        dir
    };
    if cfg!(windows) {
        let mut dirs = vec![dir.to_path_buf(), root.to_path_buf()];
        let python = root.join("python");
        if python.is_dir() {
            dirs.push(python.clone());
            dirs.push(python.join("Scripts"));
            env.insert("PYTHONHOME".into(), python.into_os_string());
        }
        prepend_env(&mut env, "PATH", dirs);
        if root.join("espeak-ng-data").is_dir() {
            env.entry("ESPEAK_DATA_PATH".into())
                .or_insert_with(|| root.join("espeak-ng-data").into_os_string());
        }
    } else {
        let lib = root.join("lib");
        if lib.is_dir() {
            prepend_env(&mut env, "LD_LIBRARY_PATH", vec![lib.clone()]);
            if lib.join("python").is_dir() {
                prepend_env(&mut env, "PYTHONPATH", vec![lib.join("python")]);
            }
        }
    }
    env
}

fn prepend_env(env: &mut BTreeMap<OsString, OsString>, key: &str, mut paths: Vec<PathBuf>) {
    let previous = env
        .get(&OsString::from(key))
        .cloned()
        .or_else(|| std::env::var_os(key));
    if let Some(previous) = previous {
        paths.extend(std::env::split_paths(&previous));
    }
    if let Ok(value) = std::env::join_paths(paths) {
        env.insert(key.into(), value);
    }
}

pub fn command_for(
    program: &Path,
    cwd: &Path,
    env: &BTreeMap<OsString, OsString>,
    args: &[String],
) -> Command {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_remove("OVMS_MODEL_REPOSITORY_PATH")
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

pub fn inspect_runtime(profile: &Profile) -> Result<(String, String), String> {
    inspect_runtime_in_bundle(profile, &bundle_directory()?)
}

pub fn inspect_runtime_in_bundle(
    profile: &Profile,
    bundle: &Path,
) -> Result<(String, String), String> {
    let program = resolve_bundled_program(bundle)?;
    let cwd = working_directory(bundle, profile.value("working_dir"))?;
    let env = runtime_environment(&program, &profile.environment()?);
    let version = bounded_output(command_for(&program, &cwd, &env, &["--version".into()]))?;
    let mut args = vec!["--help".into()];
    if profile.mode != Mode::Config && !profile.value("task").is_empty() {
        args.push(format!("--task={}", profile.value("task")));
    }
    let help = bounded_output(command_for(&program, &cwd, &env, &args))?;
    Ok((version.trim().to_owned(), help))
}

fn bounded_output(mut command: Command) -> Result<String, String> {
    let mut child = command
        .spawn()
        .map_err(|e| format!("OVMS 检测启动失败：{e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let _ = pipe.take(512 * 1024).read_to_end(&mut data);
            data
        })
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
            _ if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("OVMS 检测超时，请检查程序和运行库".into());
            }
            _ => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let mut bytes = out.join().unwrap_or_default();
    bytes.push(b'\n');
    bytes.extend(err.join().unwrap_or_default());
    let text = String::from_utf8_lossy(&bytes).to_string();
    if !status.success() {
        return Err(format!(
            "OVMS 检测退出 {status}：{}",
            text.chars().take(1500).collect::<String>()
        ));
    }
    Ok(text)
}

pub fn http_ready(host: &str, port: u16) -> Result<bool, String> {
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("地址解析失败：{e}"))?;
    let mut last = "连接失败".to_owned();
    for address in addresses.take(4) {
        match TcpStream::connect_timeout(&address, Duration::from_millis(250)) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_millis(400)))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_millis(250)))
                    .map_err(|e| e.to_string())?;
                let authority = if host.contains(':') {
                    format!("[{host}]:{port}")
                } else {
                    format!("{host}:{port}")
                };
                // Only read the status line; a body or keep-alive connection cannot stall the supervisor.
                stream.write_all(format!("GET /v2/health/ready HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n").as_bytes()).map_err(|e|e.to_string())?;
                let mut reader = std::io::BufReader::new(stream);
                let mut line = String::new();
                use std::io::BufRead;
                reader
                    .by_ref()
                    .take(4096)
                    .read_line(&mut line)
                    .map_err(|e| e.to_string())?;
                let mut words = line.split_whitespace();
                let protocol = words.next().unwrap_or("");
                let status = words.next().unwrap_or("");
                if !protocol.starts_with("HTTP/") {
                    return Err("健康检查返回了无效 HTTP 响应".into());
                }
                return Ok(status == "200");
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}
