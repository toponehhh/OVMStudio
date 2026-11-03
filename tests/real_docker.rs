//! Runs against a real local Linux Docker engine (GitHub Actions Linux runner).
use ovms_studio::config::{Backend, Mode, Profile, RuntimeSettings};
use ovms_studio::deployment::{self, PreparedRuntime};
use ovms_studio::process::{Event, Manager, Phase};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

fn wait(manager: &Manager, wanted: Phase) {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut logs = String::new();
    loop {
        for line in manager.logs.try_iter() {
            logs.push_str(&format!("{}\n", line.text));
        }
        if let Ok(Event::State { phase, detail, .. }) =
            manager.events.recv_timeout(Duration::from_millis(100))
        {
            if phase == wanted {
                return;
            }
            assert_ne!(phase, Phase::Failed, "{detail}\n{logs}");
        }
        assert!(
            Instant::now() < deadline,
            "Timeout waiting for {wanted:?}: {logs}"
        );
    }
}

fn infer(port: u16) {
    let payload = r#"{"inputs":[{"name":"input","shape":[1,3],"datatype":"FP32","data":[1,2,3]}]}"#;
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(stream, "POST /v2/models/tiny-add/infer HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let value: serde_json::Value =
        serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        value["outputs"][0]["data"],
        serde_json::json!([2.0, 4.0, 6.0])
    );
}

fn absent(ready: &PreparedRuntime, name: &str) {
    assert!(
        !Command::new(ready.docker_program.as_ref().unwrap())
            .args(["container", "inspect", name])
            .output()
            .unwrap()
            .status
            .success(),
        "Container leaked: {name}"
    );
}

fn profile(model: &Path) -> Profile {
    let mut p = Profile::default();
    p.set("model_name", "tiny-add");
    p.set("model_path", model.to_string_lossy());
    p.set("port", "0");
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    p.set("rest_port", port.to_string());
    p
}

#[test]
#[ignore = "requires a real local Linux Docker engine and internet access"]
fn official_image_pull_mount_inference_restart_config_failure_and_cleanup() {
    let project = std::env::current_dir().unwrap();
    let root = project
        .join("work/docker-integration")
        .join(deployment::unique_id());
    let application = root.join("application");
    let model = root.join("模型 with spaces,and comma");
    let data = root.join("runtime-data");
    std::fs::create_dir_all(&application).unwrap();
    std::fs::create_dir_all(model.join("1")).unwrap();
    std::fs::copy(
        project.join("examples/tiny-add/1/model.onnx"),
        model.join("1/model.onnx"),
    )
    .unwrap();
    let ready = deployment::prepare_runtime(
        &RuntimeSettings {
            backend: Backend::Docker,
            ..RuntimeSettings::default()
        },
        &data,
        true,
        &deployment::new_cancellation(),
        &|text| println!("{text}"),
    )
    .unwrap();
    let p = profile(&model);
    let port = p.port("rest_port").unwrap();
    let spec = ready.prepare_launch(&p, &application, &data).unwrap();
    let name = spec.container.as_ref().unwrap().name.clone();
    let manager = Manager::new();
    manager.start(spec.clone()).unwrap();
    wait(&manager, Phase::Ready);
    infer(port);
    manager.stop();
    wait(&manager, Phase::Idle);
    absent(&ready, &name);
    manager.start(spec).unwrap();
    wait(&manager, Phase::Ready);
    infer(port);
    drop(manager);
    absent(&ready, &name);

    let mut configured = p.clone();
    configured.mode = Mode::Config;
    let config = application.join("config.json");
    let contents = serde_json::json!({"model_config_list":[{"config":{"name":"tiny-add","base_path":model,"target_device":"CPU"}}]}).to_string();
    std::fs::write(&config, &contents).unwrap();
    configured.set("config_path", config.to_string_lossy());
    let spec = ready
        .prepare_launch(&configured, &application, &data)
        .unwrap();
    let name = spec.container.as_ref().unwrap().name.clone();
    let manager = Manager::new();
    manager.start(spec).unwrap();
    wait(&manager, Phase::Ready);
    infer(port);
    drop(manager);
    absent(&ready, &name);
    assert_eq!(std::fs::read_to_string(config).unwrap(), contents);

    std::fs::write(model.join("1/model.onnx"), b"invalid model").unwrap();
    let spec = ready.prepare_launch(&p, &application, &data).unwrap();
    let name = spec.container.as_ref().unwrap().name.clone();
    let manager = Manager::new();
    manager.start(spec).unwrap();
    wait(&manager, Phase::Failed);
    drop(manager);
    absent(&ready, &name);
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
    println!(
        "Real Docker image download, Unicode/comma mounts, inference, stop/restart, config rewriting, failure and cleanup passed."
    );
}
