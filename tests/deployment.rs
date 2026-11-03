use ovms_studio::config::{Backend, Library, Mode, Profile};
use ovms_studio::deployment::{PreparedRuntime, Release, ReleaseAsset};
use ovms_studio::{deployment, docker};
use std::io::Write;
use std::path::{Path, PathBuf};

fn root(label: &str) -> PathBuf {
    let path = std::env::current_dir()
        .unwrap()
        .join("work/deployment-tests")
        .join(format!("{label}-{}", deployment::unique_id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn release(name: &str, version: &str) -> Release {
    Release {
        tag_name: format!("v{version}"),
        draft: false,
        prerelease: false,
        assets: vec![ReleaseAsset {
            name: name.into(),
            browser_download_url: format!(
                "https://github.com/openvinotoolkit/model_server/releases/download/v{version}/{name}"
            ),
            digest: None,
            size: 0,
        }],
    }
}

#[test]
fn stable_release_supports_python_on_and_unified_windows_archives() {
    assert!(
        release("ovms_windows_2026.4.1_python_on.zip", "2026.4.1")
            .windows_asset()
            .is_ok()
    );
    assert!(
        release("ovms_windows_2026.5.zip", "2026.5")
            .windows_asset()
            .is_ok()
    );
    assert!(
        release("ovms_windows_2026.4.1_python_off.zip", "2026.4.1")
            .windows_asset()
            .is_err()
    );
    let mut invalid = release("ovms_windows_2026.5.zip", "2026.5");
    invalid.assets[0].browser_download_url =
        "https://example.test/not-the-official-release.zip".into();
    assert!(invalid.windows_asset().is_err());
    invalid.tag_name = "../outside".into();
    assert!(invalid.version().is_err());
}

#[test]
fn old_libraries_default_to_native_and_persist_docker_separately_from_models() {
    let mut library: Library =
        serde_json::from_str(r#"{"current":{"name":"existing"},"saved":{}}"#).unwrap();
    assert_eq!(library.runtime.backend, Backend::Native);
    library.runtime.backend = Backend::Docker;
    let saved: Library = serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
    assert_eq!(saved.runtime.backend, Backend::Docker);
    assert!(!saved.current.values.contains_key("executable"));
}

fn archive(path: &Path, files: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    for (name, data) in files {
        zip.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn verified_native_installation_uses_a_managed_cache_and_detects_executable_damage() {
    let data = root("native-cache");
    let filename = "ovms_windows_2026.4.1_python_on.zip";
    let downloads = data.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let package = downloads.join(filename);
    archive(
        &package,
        &[
            ("ovms/ovms.exe", b"native fixture"),
            ("ovms/bin/ovms", b"native fixture"),
            ("ovms/openvino.dll", b"fixture"),
            ("ovms/python/python.exe", b"fixture"),
            ("ovms/LICENSE", b"license"),
        ],
    );
    let mut source = release(filename, "2026.4.1");
    source.assets[0].digest = Some(format!(
        "sha256:{}",
        deployment::sha256_file(&package).unwrap()
    ));
    let ready =
        deployment::install_release(&source, &data, &deployment::new_cancellation(), &|_| {})
            .unwrap();
    assert_eq!(ready.backend, Backend::Native);
    let runtime = ready.native_root.unwrap();
    assert!(runtime.starts_with(data.join("native")));
    std::fs::write(
        runtime.join(ovms_studio::config::runtime_program_relative()),
        b"damaged",
    )
    .unwrap();
    assert!(
        deployment::cached_native(&data)
            .unwrap_err()
            .contains("损坏")
    );
    let repaired =
        deployment::install_release(&source, &data, &deployment::new_cancellation(), &|_| {})
            .unwrap();
    assert_eq!(repaired.version, "2026.4.1");
}

#[test]
fn zip_traversal_and_cancelled_installation_cannot_write_outside_the_stage() {
    let data = root("zip-paths");
    let package = data.join("bad.zip");
    archive(&package, &[("../outside.txt", b"must not escape")]);
    let stage = data.join("stage");
    std::fs::create_dir(&stage).unwrap();
    assert!(deployment::extract_zip(&package, &stage, &deployment::new_cancellation()).is_err());
    assert!(!data.join("outside.txt").exists());
    let cancelled = deployment::new_cancellation();
    cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(
        deployment::extract_zip(&package, &stage, &cancelled)
            .unwrap_err()
            .contains("取消")
    );
}

fn docker_ready() -> PreparedRuntime {
    PreparedRuntime {
        backend: Backend::Docker,
        docker_acceleration: false,
        version: "2026.4.1".into(),
        native_root: None,
        docker_program: Some("docker".into()),
        docker_image: Some("openvino/model_server:2026.4.1".into()),
        docker_image_id: Some(format!("sha256:{}", "a".repeat(64))),
    }
}

fn profile(model: &Path) -> Profile {
    let mut p = Profile::default();
    p.set("model_name", "tiny-add");
    p.set("model_path", model.to_string_lossy());
    p.set("rest_port", "18080");
    p.set("port", "19090");
    p
}

#[test]
fn docker_maps_paths_ports_and_secrets_without_shell_interpolation() {
    let application = root("application");
    let model = root("模型 with spaces,and comma");
    let mut p = profile(&model);
    p.set(
        "environment",
        r#"{"HF_TOKEN":"must-stay-out-of-arguments"}"#,
    );
    p.set("cache_dir", application.join("cache").to_string_lossy());
    let spec = docker::launch(
        &p,
        &docker_ready(),
        &application,
        &application.join("runtime-data"),
    )
    .unwrap();
    assert!(spec.args.contains(&"--rest_bind_address=0.0.0.0".into()));
    assert!(spec.args.contains(&"127.0.0.1:18080:18080".into()));
    assert!(
        spec.args
            .iter()
            .any(|arg| arg.starts_with("--model_path=/ovmstudio/data/"))
    );
    assert!(
        spec.args
            .iter()
            .any(|arg| arg.contains("\"source=") && arg.contains(",readonly"))
    );
    assert!(!spec.args.join(" ").contains("must-stay-out-of-arguments"));
    assert_eq!(
        spec.env.get(&std::ffi::OsString::from("HF_TOKEN")).unwrap(),
        "must-stay-out-of-arguments"
    );
    assert!(!spec.preview().contains("must-stay-out-of-arguments"));
    assert!(
        spec.container
            .as_ref()
            .unwrap()
            .name
            .starts_with("ovmstudio-")
    );
    assert!(application.join("cache").is_dir());
    assert_eq!(spec.profile.endpoint(), "http://127.0.0.1:18080");
    assert!(spec.args.iter().all(|arg| !arg.contains("\\\\?\\")));
}

#[test]
fn docker_rewrites_config_paths_without_modifying_the_original_json() {
    let application = root("config-application");
    let model = root("external-model");
    let mut p = profile(&model);
    p.mode = Mode::Config;
    let original = application.join("config.json");
    let content = serde_json::json!({"model_config_list":[{"config":{"name":"tiny-add","base_path":model,"target_device":"CPU"}}]}).to_string();
    std::fs::write(&original, &content).unwrap();
    p.set("config_path", original.to_string_lossy());
    let data = application.join("data");
    let spec = docker::launch(&p, &docker_ready(), &application, &data).unwrap();
    assert_eq!(std::fs::read_to_string(original).unwrap(), content);
    let generated = std::fs::read_dir(data.join("container-configs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
        .join("config.json");
    let mapped: serde_json::Value =
        serde_json::from_slice(&std::fs::read(generated).unwrap()).unwrap();
    assert!(
        mapped["model_config_list"][0]["config"]["base_path"]
            .as_str()
            .unwrap()
            .starts_with("/ovmstudio/data/")
    );
    assert!(
        spec.args
            .iter()
            .any(|arg| arg.starts_with("--config_path=/workspace/"))
    );
    assert!(!spec.args.iter().any(|arg| arg.starts_with("--model_path=")));
}

#[test]
fn docker_rejects_missing_images_unsupported_accelerators_and_client_env_overrides() {
    let application = root("invalid-docker");
    let mut p = profile(&application);
    let mut ready = docker_ready();
    ready.docker_image_id = Some("not-a-verified-image-id".into());
    assert!(docker::launch(&p, &ready, &application, &application).is_err());
    p.set("target_device", "GPU");
    assert!(
        docker::launch(&p, &docker_ready(), &application, &application)
            .unwrap_err()
            .contains("加速器镜像")
    );
    p.set("target_device", "CPU");
    p.set(
        "environment",
        r#"{"DOCKER_HOST":"tcp://some-other-host:2375"}"#,
    );
    assert!(
        docker::launch(&p, &docker_ready(), &application, &application)
            .unwrap_err()
            .contains("DOCKER_")
    );
}

#[test]
fn docker_acceleration_follows_active_devices_and_generates_platform_mappings() {
    let application = root("accelerator-plan");
    let mut p = profile(&application);
    p.set("draft_device", "GPU");
    p.set("task", "embeddings");
    assert!(
        docker::requested_devices(&p, &application)
            .unwrap()
            .is_empty()
    );
    p.set("target_device", "GPU");
    let mut ready = docker_ready();
    ready.docker_acceleration = true;
    ready.docker_image = Some("openvino/model_server:2026.4.1-gpu".into());
    let spec = docker::launch(&p, &ready, &application, &application.join("runtime-data")).unwrap();
    assert!(
        spec.args
            .iter()
            .any(|value| value == "/dev/dri" || value == "/dev/dxg")
    );
    if cfg!(windows) {
        assert!(spec.args.contains(&"/usr/lib/wsl:/usr/lib/wsl:ro".into()));
    }
    p.set("target_device", "NPU");
    let result = docker::launch(&p, &ready, &application, &application.join("runtime-data"));
    if cfg!(windows) {
        assert!(result.unwrap_err().contains("Linux 宿主"));
    } else {
        assert!(result.unwrap().args.contains(&"/dev/accel".into()));
    }
}
