use ovms_studio::config::{Mode, Profile};
use ovms_studio::process::{Event, Manager, Phase};
use ovms_studio::runtime::{LaunchSpec, http_ready, inspect_native_runtime};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn profile() -> Profile {
    let mut p = Profile::default();
    p.set(
        "working_dir",
        std::env::current_dir().unwrap().to_string_lossy(),
    );
    p.set(
        "model_path",
        std::env::current_dir()
            .unwrap()
            .join("examples/tiny-add")
            .to_string_lossy(),
    );
    p.set("model_name", "tiny-add");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    p.set(
        "rest_port",
        listener.local_addr().unwrap().port().to_string(),
    );
    p.set("port", "0");
    p
}
fn bundle() -> PathBuf {
    PathBuf::from(
        std::env::var("OVMS_TEST_BUNDLE")
            .expect("Set OVMS_TEST_BUNDLE to the application bundle root"),
    )
}
fn prepare(profile: &Profile) -> LaunchSpec {
    LaunchSpec::prepare_native(profile, &bundle(), &std::env::current_dir().unwrap()).unwrap()
}
fn wait(manager: &Manager, wanted: Phase) -> String {
    let end = Instant::now() + Duration::from_secs(25);
    let mut output = String::new();
    loop {
        for line in manager.logs.try_iter() {
            output.push_str(&line.text);
            output.push('\n');
        }
        if let Ok(Event::State { phase, detail, .. }) =
            manager.events.recv_timeout(Duration::from_millis(100))
        {
            if phase == wanted {
                return detail;
            }
            assert_ne!(phase, Phase::Failed, "{detail}\n{output}");
        }
        assert!(
            Instant::now() < end,
            "Timed out waiting for {wanted:?}\n{output}"
        );
    }
}
fn infer(port: u16) {
    let payload = r#"{"inputs":[{"name":"input","shape":[1,3],"datatype":"FP32","data":[1,2,3]}]}"#;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream,"POST /v2/models/tiny-add/infer HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",payload.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let body = response.split_once("\r\n\r\n").unwrap().1;
    let value: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        value["outputs"][0]["data"],
        serde_json::json!([2.0, 4.0, 6.0])
    );
}

#[test]
#[ignore = "requires bundled OVMS: set OVMS_TEST_BUNDLE"]
fn real_model_load_infer_stop_restart_and_drop() {
    let p = profile();
    let port = p.port("rest_port").unwrap();
    let manager = Manager::new();
    manager.start(prepare(&p)).unwrap();
    wait(&manager, Phase::Ready);
    infer(port);
    manager.stop();
    wait(&manager, Phase::Idle);
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    manager.start(prepare(&p)).unwrap();
    wait(&manager, Phase::Ready);
    infer(port);
    drop(manager);
    assert!(!http_ready("127.0.0.1", port).unwrap_or(false));
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
}

#[test]
#[ignore = "requires bundled OVMS: set OVMS_TEST_BUNDLE"]
fn single_model_file_with_spaces_in_its_path() {
    let mut p = profile();
    let path = std::env::current_dir()
        .unwrap()
        .join("work/model with spaces/tiny model.onnx");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy("examples/tiny-add/1/model.onnx", &path).unwrap();
    p.set("model_path", path.to_string_lossy());
    let manager = Manager::new();
    manager.start(prepare(&p)).unwrap();
    wait(&manager, Phase::Ready);
    infer(p.port("rest_port").unwrap());
    manager.stop();
    wait(&manager, Phase::Idle);
}

#[test]
#[ignore = "requires bundled OVMS: set OVMS_TEST_BUNDLE"]
fn occupied_port_is_rejected_before_spawn() {
    let mut p = profile();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    p.set(
        "rest_port",
        listener.local_addr().unwrap().port().to_string(),
    );
    let manager = Manager::new();
    manager.start(prepare(&p)).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(event) = manager.events.recv_timeout(Duration::from_millis(100)) {
            match event {
                Event::Started(_) => panic!("Spawned on an occupied port"),
                Event::State {
                    phase: Phase::Failed,
                    detail,
                    ..
                } => {
                    assert!(detail.contains("无法监听"));
                    break;
                }
                _ => {}
            }
        }
        assert!(Instant::now() < end);
    }
}

#[test]
#[ignore = "requires bundled OVMS: set OVMS_TEST_BUNDLE"]
fn multi_model_configuration_uses_real_config_json() {
    let mut p = profile();
    p.mode = Mode::Config;
    p.set(
        "config_path",
        std::env::current_dir()
            .unwrap()
            .join("examples/multi-model-config.json")
            .to_string_lossy(),
    );
    let manager = Manager::new();
    manager.start(prepare(&p)).unwrap();
    wait(&manager, Phase::Ready);
    infer(p.port("rest_port").unwrap());
    manager.stop();
    wait(&manager, Phase::Idle);
}

#[test]
#[ignore = "requires bundled OVMS: set OVMS_TEST_BUNDLE"]
fn local_help_confirms_embedding_flag_and_version() {
    let mut p = profile();
    p.set("task", "embeddings");
    let (version, help) =
        inspect_native_runtime(&p, &bundle(), &std::env::current_dir().unwrap()).unwrap();
    assert!(version.contains("2026.4.1"), "{version}");
    assert!(help.contains("--max_length"));
    assert!(!help.contains("--max_legth"));
}

#[test]
#[ignore = "requires bundled OVMS: set OVMS_TEST_BUNDLE"]
fn invalid_model_reports_exit_instead_of_ready() {
    let mut p = profile();
    let dir = PathBuf::from("work/bad-model/1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("model.onnx"), "invalid").unwrap();
    p.set(
        "model_path",
        std::env::current_dir()
            .unwrap()
            .join("work/bad-model")
            .to_string_lossy(),
    );
    let manager = Manager::new();
    manager.start(prepare(&p)).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        for _ in manager.logs.try_iter() {}
        if let Ok(Event::State { phase, .. }) =
            manager.events.recv_timeout(Duration::from_millis(100))
        {
            assert_ne!(phase, Phase::Ready);
            if phase == Phase::Failed {
                break;
            }
        }
        assert!(Instant::now() < end);
    }
}
