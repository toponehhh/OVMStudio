use ovms_studio::config::{Mode, Profile, connect_host, display_command, save_json};
use ovms_studio::runtime::{LaunchSpec, http_ready, resolve_bundled_program};
use std::io::{Read, Write};
use std::net::TcpListener;

fn local() -> Profile {
    let mut p = Profile::default();
    p.set("model_path", "models/resnet");
    p
}

#[test]
fn legacy_profile_runtime_path_is_discarded() {
    let mut p = local();
    p.set("executable", "C:/old-install/ovms.exe");
    p.upgrade().unwrap();
    assert!(!p.values.contains_key("executable"));
    assert!(
        !ovms_studio::config::fields()
            .iter()
            .any(|f| f.key == "executable")
    );
    assert!(!p.preview().contains("old-install"));
    assert!(
        p.preview()
            .starts_with(ovms_studio::config::bundled_program_relative())
    );
}

#[test]
fn bundle_location_controls_runtime_and_default_working_directory() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work/contract-bundle with spaces");
    let program = root.join(ovms_studio::config::bundled_program_relative());
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::write(&program, b"resolver fixture only").unwrap();
    let mut p = local();
    p.set(
        "model_path",
        std::env::current_dir()
            .unwrap()
            .join("examples/tiny-add")
            .to_string_lossy(),
    );
    p.set("executable", "C:/legacy/runtime/ignored.exe");
    let spec = LaunchSpec::prepare_in_bundle(&p, &root).unwrap();
    assert_eq!(spec.program, std::fs::canonicalize(&program).unwrap());
    assert_eq!(spec.cwd, std::fs::canonicalize(&root).unwrap());
}

#[test]
fn missing_bundled_runtime_requests_the_complete_application_package() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work/missing-bundle-runtime");
    std::fs::create_dir_all(&root).unwrap();
    let error = resolve_bundled_program(&root).unwrap_err();
    assert!(error.contains("重新解压完整"));
    assert!(!error.contains("请选择"));
}

#[test]
fn configuration_modes_never_mix_model_arguments() {
    let mut p = local();
    p.set("source_model", "OpenVINO/Qwen3-0.6B-int4-ov");
    p.set("config_path", "config.json");
    p.set("model_repository_path", "models");
    let args = p.arguments().unwrap();
    assert!(args.iter().any(|a| a.starts_with("--model_path=")));
    assert!(
        !args
            .iter()
            .any(|a| a.starts_with("--source_model=") || a.starts_with("--config_path="))
    );
    p.mode = Mode::HuggingFace;
    p.set("task", "text_generation");
    let args = p.arguments().unwrap();
    assert!(args.iter().any(|a| a.starts_with("--source_model=")));
    assert!(!args.iter().any(|a| a == "--pull"
        || a.starts_with("--model_path=")
        || a.starts_with("--config_path=")));
    p.mode = Mode::Config;
    let args = p.arguments().unwrap();
    for flag in [
        "model_path",
        "model_name",
        "source_model",
        "task",
        "target_device",
        "plugin_config",
        "enable_prefix_caching",
    ] {
        assert!(!args.iter().any(|a| a.starts_with(&format!("--{flag}="))));
    }
    assert!(args.contains(&"--config_path=config.json".into()));
}

#[test]
fn shape_takes_precedence_over_batch_and_inactive_fields_are_ignored() {
    let mut p = local();
    p.set("batch_size", "8");
    p.set("shape", "(1,3,224,224)");
    p.set("cache_size", "bad");
    let args = p.arguments().unwrap();
    assert!(
        !args
            .iter()
            .any(|a| a.starts_with("--batch_size=") || a.starts_with("--cache_size="))
    );
    assert!(args.iter().any(|a| a.starts_with("--shape=")));
    p.set("task", "text_generation");
    assert!(p.arguments().unwrap_err().contains("KV Cache"));
}

#[test]
fn boolean_false_is_explicit_and_prefix_cache_can_be_disabled() {
    let mut p = local();
    p.set("task", "text_generation");
    p.set("enable_prefix_caching", "false");
    let args = p.arguments().unwrap();
    assert!(args.contains(&"--enable_prefix_caching=false".into()));
    assert!(args.contains(&"--metrics_enable=false".into()));
}

#[test]
fn imported_whitespace_does_not_change_task_scope_or_boolean_meaning() {
    let mut p = local();
    p.set("task", " embeddings ");
    p.set("max_length", " 512 ");
    p.set("normalize", " false ");
    let args = p.arguments().unwrap();
    assert!(args.contains(&"--task=embeddings".into()));
    assert!(args.contains(&"--max_length=512".into()));
    assert!(args.contains(&"--normalize=false".into()));
    assert!(!args.iter().any(|a| a.starts_with("--batch_size=")));
}

#[test]
fn endpoints_and_metrics_are_validated_together() {
    let mut p = local();
    p.set("rest_port", "9000");
    assert!(p.arguments().is_err());
    p.set("rest_port", "70000");
    assert!(p.arguments().is_err());
    p.set("rest_port", "0");
    p.set("port", "0");
    assert!(p.arguments().is_err());
    p.set("port", "9000");
    p.set("metrics_enable", "true");
    assert!(p.arguments().is_err());
    p.set("rest_port", "8000");
    assert!(p.arguments().is_ok());
    p.set("rest_bind_address", "::");
    assert_eq!(p.endpoint(), "http://[::1]:8000");
    assert_eq!(connect_host("0.0.0.0,127.0.0.1"), "127.0.0.1");
}

#[test]
fn extra_args_cannot_override_ui_or_turn_launch_into_one_shot_commands() {
    let mut p = local();
    for flag in [
        "--help",
        "--pull=false",
        "--configure",
        "--remove_from_config",
        "--rest_port=1",
        "--task",
        "-h",
        "--source_model=x",
    ] {
        p.set("extra_args", serde_json::to_string(&vec![flag]).unwrap());
        assert!(p.arguments().is_err(), "{flag}");
    }
    let literal = "a path with spaces & whoami > file";
    p.set(
        "extra_args",
        serde_json::to_string(&vec!["--future_option", literal]).unwrap(),
    );
    let args = p.arguments().unwrap();
    assert_eq!(args.last().unwrap(), literal);
    assert_eq!(args.iter().filter(|s| s.as_str() == literal).count(), 1);
    assert!(!display_command("ovms", &args).is_empty());
}

#[test]
fn native_parameter_types_and_dependencies_are_checked() {
    let mut p = local();
    p.set("mean", "1,2,3");
    assert!(p.arguments().unwrap_err().contains("layout"));
    p.set("layout", "NCHW");
    assert!(p.arguments().is_ok());
    p.set("plugin_config", "[]");
    assert!(p.arguments().is_err());
    p.set("plugin_config", "{}");
    p.set("model_version_policy", r#"{"latest":{"num_versions":0}}"#);
    assert!(p.arguments().is_err());
    p.set("model_version_policy", r#"{"specific":{"versions":[1,2]}}"#);
    assert!(p.arguments().is_ok());
    p.set("task", "text_generation");
    p.set("cache_size", "0.5");
    assert!(p.arguments().is_err());
    p.set("cache_size", "0");
    p.set("draft_source_model", "org/draft");
    p.set("draft_model_path", "draft");
    assert!(p.arguments().is_err());
    p.set("draft_source_model", "");
    p.set("enable_tool_guided_generation", "true");
    assert!(p.arguments().is_err());
    p.set("tool_parser", "hermes3");
    assert!(p.arguments().is_ok());
}

#[test]
fn embeddings_use_source_spelling_and_task_switch_drops_old_options() {
    let mut p = local();
    p.set("task", "embeddings");
    p.set("max_length", "512");
    p.set("max_num_seqs", "32");
    let args = p.arguments().unwrap();
    assert!(args.contains(&"--max_length=512".into()));
    assert!(!args.iter().any(|a| a.starts_with("--max_num_seqs=")));
    p.set("task", "text_generation");
    let args = p.arguments().unwrap();
    assert!(!args.iter().any(|a| a.starts_with("--max_length=")));
}

#[test]
fn image_generation_accepts_float_guidance_and_requires_static_resolution() {
    let mut p = local();
    p.set("task", "image_generation");
    p.set("guidance_scale", "7.5");
    assert!(p.arguments().is_err());
    p.set("resolution", "512x512");
    assert!(p.arguments().is_ok());
    p.set("resolution", "512x512 1024x1024");
    assert!(p.arguments().is_err());
    p.set("resolution", "512x512");
    p.set("guidance_scale", "NaN");
    assert!(p.arguments().is_err());
}

#[test]
fn openvino_hf_models_do_not_accept_conversion_precision() {
    let mut p = local();
    p.mode = Mode::HuggingFace;
    p.set("source_model", "OpenVINO/Qwen3-0.6B-int4-ov");
    p.set("model_repository_path", "models");
    p.set("weight-format", "int4");
    assert!(p.arguments().is_err());
    p.set("weight-format", "");
    assert!(p.arguments().is_ok());
}

#[test]
fn environment_is_typed_and_profile_versions_are_not_silently_downgraded() {
    let mut p = local();
    p.set("environment", r#"{"HF_ENDPOINT":42}"#);
    assert!(p.arguments().is_err());
    p.set("environment", r#"{"HF_ENDPOINT":"https://example.test"}"#);
    assert!(p.arguments().is_ok());
    p.set("environment", r#"{"BAD=KEY":"x"}"#);
    assert!(p.arguments().is_err());
    p.schema_version = 2;
    assert!(p.upgrade().is_err());
}

#[test]
fn saving_existing_configuration_is_recoverable_and_preserves_unicode() {
    let dir = std::env::current_dir().unwrap().join("work/tests-profile");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("配置.json");
    let p = local();
    save_json(&path, &p).unwrap();
    let mut p = p;
    p.name = "中文路径与模型".into();
    save_json(&path, &p).unwrap();
    let restored: Profile = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(restored.name, p.name);
}

#[test]
fn readiness_requires_http_200_and_uses_the_ovms_health_path() {
    for (status, expected) in [
        ("HTTP/1.1 200 OK", true),
        ("HTTP/1.1 503 Service Unavailable", false),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = [0; 1024];
            let n = stream.read(&mut bytes).unwrap();
            assert!(
                String::from_utf8_lossy(&bytes[..n]).starts_with("GET /v2/health/ready HTTP/1.1")
            );
            write!(
                stream,
                "{status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
        });
        assert_eq!(http_ready("127.0.0.1", port).unwrap(), expected);
        server.join().unwrap();
    }
}
