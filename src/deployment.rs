//! User-managed execution environments. No OVMS binaries are shipped with the UI.
use crate::config::{self, Backend, RuntimeSettings};
use crate::runtime::{self, LaunchSpec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const LATEST_RELEASE: &str =
    "https://api.github.com/repos/openvinotoolkit/model_server/releases/latest";
const DOWNLOAD_PREFIX: &str = "https://github.com/openvinotoolkit/model_server/releases/download/";
pub const DOCKER_INSTALL_URL: &str = "https://docs.docker.com/desktop/";
pub type Cancellation = Arc<AtomicBool>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

impl Release {
    pub fn version(&self) -> Result<&str, String> {
        let version = self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name);
        if self.draft
            || self.prerelease
            || version.is_empty()
            || version.contains("..")
            || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        {
            return Err("官方最新稳定版本信息无效".into());
        }
        Ok(version)
    }

    pub fn windows_asset(&self) -> Result<&ReleaseAsset, String> {
        let version = self.version()?;
        // 2026.4 uses python_on/off; 2026.5+ merges them into one package.
        let candidates = [
            format!("ovms_windows_{version}_python_on.zip"),
            format!("ovms_windows_{version}.zip"),
        ];
        let asset = candidates
            .iter()
            .find_map(|name| self.assets.iter().find(|a| &a.name == name))
            .ok_or("最新稳定版尚未提供 Windows x64 包，请使用 Docker 模式")?;
        if !asset
            .browser_download_url
            .starts_with(&format!("{DOWNLOAD_PREFIX}{}/", self.tag_name))
        {
            return Err("运行包下载地址不属于 OVMS 官方 Release".into());
        }
        Ok(asset)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreparedRuntime {
    pub backend: Backend,
    #[serde(default)]
    pub docker_acceleration: bool,
    pub version: String,
    pub native_root: Option<PathBuf>,
    pub docker_program: Option<PathBuf>,
    pub docker_image: Option<String>,
    pub docker_image_id: Option<String>,
}

impl PreparedRuntime {
    pub fn prepare_launch(
        &self,
        profile: &config::Profile,
        application: &Path,
        data: &Path,
    ) -> Result<LaunchSpec, String> {
        match self.backend {
            Backend::Native => LaunchSpec::prepare_native(
                profile,
                self.native_root.as_deref().ok_or("本机运行时尚未准备")?,
                application,
            ),
            Backend::Docker => crate::docker::launch(profile, self, application, data),
        }
    }
    pub fn description(&self) -> String {
        match self.backend {
            Backend::Native => format!("本机 OVMS {} · 已下载到用户缓存", self.version),
            Backend::Docker => format!(
                "Docker · {} · 镜像已准备",
                self.docker_image.as_deref().unwrap_or("OVMS")
            ),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NativeInstallation {
    pub version: String,
    pub archive_sha256: String,
    pub executable_sha256: String,
}

pub fn new_cancellation() -> Cancellation {
    Arc::new(AtomicBool::new(false))
}

pub fn check_cancel(cancel: &Cancellation) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("运行环境准备已取消".into())
    } else {
        Ok(())
    }
}

pub fn unique_id() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

fn client() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("OVMStudio/", env!("CARGO_PKG_VERSION")))
        .timeout_global(Some(Duration::from_secs(1200)))
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .build()
        .into()
}

pub fn latest_release() -> Result<Release, String> {
    let mut response = client()
        .get(LATEST_RELEASE)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("无法检查 OVMS 最新版本：{e}。请检查网络，稍后重试。"))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let release: Release =
        serde_json::from_slice(&bytes).map_err(|e| format!("官方版本信息解析失败：{e}"))?;
    release.version()?;
    Ok(release)
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = [0; 128 * 1024];
    loop {
        let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn cached_native(data: &Path) -> Result<PreparedRuntime, String> {
    let raw = fs::read(data.join("native-current.json"))
        .map_err(|_| "本机 OVMS 尚未下载；启动服务或点击“准备最新运行环境”即可下载".to_string())?;
    let info: NativeInstallation =
        serde_json::from_slice(&raw).map_err(|e| format!("运行时记录损坏：{e}"))?;
    validate_version(&info.version)?;
    let root = data.join("native").join(&info.version);
    let program = runtime::resolve_native_program(&root)?;
    if sha256_file(&program)? != info.executable_sha256 {
        return Err("已下载的 OVMS 文件损坏，请在设置中重新下载最新运行环境".into());
    }
    Ok(PreparedRuntime {
        backend: Backend::Native,
        docker_acceleration: false,
        version: info.version,
        native_root: Some(root),
        docker_program: None,
        docker_image: None,
        docker_image_id: None,
    })
}

fn validate_version(version: &str) -> Result<(), String> {
    if version.is_empty()
        || version.contains("..")
        || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err("运行时版本格式无效".into());
    }
    Ok(())
}

pub fn prepare_runtime(
    settings: &RuntimeSettings,
    data: &Path,
    force_update: bool,
    cancel: &Cancellation,
    progress: &dyn Fn(String),
) -> Result<PreparedRuntime, String> {
    fs::create_dir_all(data).map_err(|e| format!("无法创建运行时目录：{e}"))?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(data.join("prepare.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock()
        .map_err(|_| "另一个 OVMStudio 正在准备运行环境，请稍后重试".to_string())?;
    check_cancel(cancel)?;
    match settings.backend {
        Backend::Native => {
            if !cfg!(all(windows, target_arch = "x86_64")) {
                return Err("本机自动安装目前支持 Windows x64；此平台请使用 Docker 模式".into());
            }
            if !force_update && let Ok(ready) = cached_native(data) {
                return Ok(ready);
            }
            progress("正在检查官方最新稳定版 OVMS…".into());
            let release = latest_release()?;
            install_release(&release, data, cancel, progress)
        }
        Backend::Docker => {
            progress("正在检查 Docker 客户端与引擎…".into());
            let program = crate::docker::detect()?;
            let record = data.join(if settings.docker_acceleration {
                "docker-gpu-current.json"
            } else {
                "docker-cpu-current.json"
            });
            if !force_update
                && let Ok(raw) = fs::read(&record)
                && let Ok(ready) = serde_json::from_slice::<PreparedRuntime>(&raw)
                && ready.backend == Backend::Docker
                && ready.docker_acceleration == settings.docker_acceleration
                && let Some(image) = ready.docker_image_id.as_deref()
                && crate::docker::image_exists(&program, image)
            {
                return Ok(PreparedRuntime {
                    docker_program: Some(program),
                    ..ready
                });
            }
            progress("Docker 已就绪，正在检查 OVMS 最新稳定版…".into());
            let release = latest_release()?;
            let image = format!(
                "openvino/model_server:{}{}",
                release.version()?,
                if settings.docker_acceleration {
                    "-gpu"
                } else {
                    ""
                }
            );
            let id = crate::docker::pull_image(&program, &image, cancel, progress)?;
            let ready = PreparedRuntime {
                backend: Backend::Docker,
                docker_acceleration: settings.docker_acceleration,
                version: release.version()?.into(),
                native_root: None,
                docker_program: Some(program),
                docker_image: Some(image),
                docker_image_id: Some(id),
            };
            config::save_json(&record, &ready)?;
            Ok(ready)
        }
    }
}

pub fn install_release(
    release: &Release,
    data: &Path,
    cancel: &Cancellation,
    progress: &dyn Fn(String),
) -> Result<PreparedRuntime, String> {
    let asset = release.windows_asset()?;
    let version = release.version()?;
    if let Ok(cached) = cached_native(data)
        && cached.version == version
    {
        return Ok(cached);
    }
    let expected = match asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
    {
        Some(digest) => digest.to_owned(),
        None => {
            let name = format!("{}.sha256", asset.name);
            let checksum = release
                .assets
                .iter()
                .find(|a| a.name == name)
                .ok_or("官方运行包缺少 SHA-256 校验值")?;
            if !checksum
                .browser_download_url
                .starts_with(&format!("{DOWNLOAD_PREFIX}{}/", release.tag_name))
            {
                return Err("非官方校验文件地址".into());
            }
            let text = client()
                .get(&checksum.browser_download_url)
                .call()
                .map_err(|e| e.to_string())?
                .body_mut()
                .read_to_string()
                .map_err(|e| e.to_string())?;
            text.split_whitespace().next().unwrap_or("").to_owned()
        }
    };
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("官方 SHA-256 格式无效".into());
    }
    fs::create_dir_all(data.join("downloads")).map_err(|e| e.to_string())?;
    let archive = data.join("downloads").join(&asset.name);
    if !archive.is_file() || sha256_file(&archive)? != expected.to_lowercase() {
        progress(format!(
            "正在下载 OVMS {version}（{:.0} MB）…",
            asset.size as f64 / 1_000_000.0
        ));
        download(
            &asset.browser_download_url,
            &archive,
            asset.size,
            cancel,
            progress,
        )?;
    }
    check_cancel(cancel)?;
    progress("正在校验运行包 SHA-256…".into());
    if sha256_file(&archive)? != expected.to_lowercase() {
        return Err("运行包 SHA-256 不匹配，已停止安装；请重新下载".into());
    }
    let parent = data.join("native");
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let staging = parent.join(format!(".install-{}", unique_id()));
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    progress("正在解压已校验的 OVMS 运行包…".into());
    extract_zip(&archive, &staging, cancel)?;
    let program = runtime::resolve_native_program(&staging)?;
    for required in [
        "ovms/openvino.dll",
        "ovms/python/python.exe",
        "ovms/LICENSE",
    ] {
        if !staging.join(required).is_file() {
            return Err(format!("官方运行包缺少 {required}"));
        }
    }
    let info = NativeInstallation {
        version: version.into(),
        archive_sha256: expected.to_lowercase(),
        executable_sha256: sha256_file(&program)?,
    };
    let destination = parent.join(version);
    if destination.exists() {
        // Preserve an older/broken installation; never recursively remove user data.
        fs::rename(
            &destination,
            parent.join(format!(".backup-{version}-{}", unique_id())),
        )
        .map_err(|e| e.to_string())?;
    }
    fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
    config::save_json(&data.join("native-current.json"), &info)?;
    cached_native(data)
}

fn download(
    url: &str,
    destination: &Path,
    expected_size: u64,
    cancel: &Cancellation,
    progress: &dyn Fn(String),
) -> Result<(), String> {
    let partial = destination.with_extension("zip.partial");
    let mut response = client()
        .get(url)
        .call()
        .map_err(|e| format!("下载失败：{e}"))?;
    let mut reader = response.body_mut().as_reader();
    let mut file = File::create(&partial).map_err(|e| e.to_string())?;
    let mut bytes = [0; 128 * 1024];
    let mut total = 0u64;
    let mut notified = 0u64;
    loop {
        check_cancel(cancel)?;
        let n = reader
            .read(&mut bytes)
            .map_err(|e| format!("下载中断：{e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > 2 * 1024 * 1024 * 1024 || (expected_size > 0 && total > expected_size) {
            return Err("运行包大小超出官方记录".into());
        }
        file.write_all(&bytes[..n]).map_err(|e| e.to_string())?;
        if total / (8 * 1024 * 1024) > notified {
            notified = total / (8 * 1024 * 1024);
            progress(format!(
                "已下载 {:.1} / {:.1} MB",
                total as f64 / 1_000_000.0,
                expected_size as f64 / 1_000_000.0
            ));
        }
    }
    if expected_size > 0 && total != expected_size {
        return Err("运行包下载不完整，请重试".into());
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(partial, destination).map_err(|e| e.to_string())
}

pub fn extract_zip(
    archive: &Path,
    destination: &Path,
    cancel: &Cancellation,
) -> Result<(), String> {
    let mut source = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut expanded = 0u64;
    if source.len() > 100_000 {
        return Err("运行包文件数超出限制".into());
    }
    for index in 0..source.len() {
        check_cancel(cancel)?;
        let mut entry = source.by_index(index).map_err(|e| e.to_string())?;
        let enclosed = entry.enclosed_name().ok_or("运行包包含不安全的路径")?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("运行包包含符号链接".into());
        }
        let target = destination.join(enclosed);
        expanded = expanded.checked_add(entry.size()).ok_or("运行包大小溢出")?;
        if expanded > 6 * 1024 * 1024 * 1024 {
            return Err("运行包解压大小超出限制".into());
        }
        if entry.is_dir() {
            fs::create_dir_all(target).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut file = File::create(target).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file)
                .map_err(|e| format!("运行包解压或 CRC 校验失败：{e}"))?;
        }
    }
    Ok(())
}
