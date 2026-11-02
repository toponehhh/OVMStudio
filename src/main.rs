#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use ovms_studio::config::{self, Library, Mode, Profile};
use ovms_studio::process::{Event, Manager, Phase};
use ovms_studio::runtime::{self, LaunchSpec};
use slint::{ComponentHandle, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{atomic::Ordering, mpsc};
use std::time::{Duration, Instant};

slint::include_modules!();

enum TaskResult {
    Picked(String, Option<PathBuf>),
    Imported(Result<Option<Profile>, String>),
    Written(Result<Option<String>, String>),
    Inspected(Result<(String, String), String>),
}

struct App {
    library: Library,
    manager: Manager,
    active: Option<Profile>,
    launched_at: Option<Instant>,
    logs: VecDeque<String>,
    boot: Instant,
    filter: String,
    task_tx: mpsc::Sender<TaskResult>,
    task_rx: mpsc::Receiver<TaskResult>,
    persist: bool,
    state_file: PathBuf,
}

impl App {
    fn refresh(&self, ui: &MainWindow, rebuild: bool) {
        let profile = &self.library.current;
        ui.set_mode(profile.mode.index());
        ui.set_profile_name(profile.name.clone().into());
        ui.set_validation_error(profile.validate_values().err().unwrap_or_default().into());
        let displayed = self.active.as_ref().unwrap_or(profile);
        ui.set_command_preview(displayed.preview().into());
        ui.set_rest_endpoint(displayed.endpoint().into());
        ui.set_grpc_endpoint(displayed.grpc_endpoint().into());
        ui.set_rest_enabled(displayed.port("rest_port").is_ok_and(|p| p > 0));
        ui.set_model_label(if displayed.mode == Mode::Config {
            "多模型配置".into()
        } else {
            displayed.value("model_name").into()
        });
        ui.set_device_label(if displayed.mode == Mode::Config {
            "配置定义".into()
        } else if displayed.value("target_device").is_empty() {
            "AUTO".into()
        } else {
            displayed.value("target_device").into()
        });
        ui.set_task_label(if displayed.mode == Mode::Config {
            "由 config.json 定义".into()
        } else if displayed.value("task").is_empty() {
            "传统推理 / 自动识别".into()
        } else {
            displayed.value("task").into()
        });
        ui.set_api_path(api_path(displayed).into());
        ui.set_has_profiles(!self.library.saved.is_empty());
        let names = std::iter::once(SharedString::from("载入已保存的配置…"))
            .chain(self.library.saved.keys().map(|s| s.as_str().into()))
            .collect::<Vec<_>>();
        ui.set_saved_names(Rc::new(VecModel::from(names)).into());
        if !rebuild {
            return;
        }
        let mut previous_group = "";
        let filter = self.filter.to_lowercase();
        let mut definitions = config::fields();
        if ui.get_page() == 0 {
            definitions.sort_by_key(|f| if f.group == "模型来源" { 0 } else { 1 });
        }
        let rows = definitions
            .into_iter()
            .filter(|f| f.page == ui.get_page() && profile.applies(f))
            .filter(|f| {
                filter.is_empty()
                    || ui.get_page() != 3
                    || format!("{} {} {} {}", f.key, f.label, f.hint, f.group)
                        .to_lowercase()
                        .contains(&filter)
            })
            .map(|f| {
                let show_group = previous_group != f.group;
                previous_group = f.group;
                let choice = f
                    .choices
                    .iter()
                    .position(|v| *v == profile.value(f.key))
                    .unwrap_or(0) as i32;
                FieldData {
                    key: f.key.into(),
                    label: f.label.into(),
                    hint: f.hint.into(),
                    placeholder: f.placeholder.into(),
                    value: profile.value(f.key).into(),
                    kind: f.kind.index(),
                    choice,
                    group: f.group.into(),
                    show_group,
                    choices: Rc::new(VecModel::from(
                        f.choices
                            .iter()
                            .map(|s| {
                                if s.is_empty() {
                                    "默认 / 自动".into()
                                } else {
                                    (*s).into()
                                }
                            })
                            .collect::<Vec<SharedString>>(),
                    ))
                    .into(),
                }
            })
            .collect::<Vec<_>>();
        ui.set_fields(Rc::new(VecModel::from(rows)).into());
    }

    fn navigate(&mut self, ui: &MainWindow, page: i32) {
        ui.set_page(page);
        let (title, description) = match page {
            1 => (
                "为服务配置 Endpoint",
                "设置 REST、gRPC 和并发参数，让客户端连接你的模型。",
            ),
            2 => (
                "让推理更适合你的设备",
                "选择计算设备，按模型任务调整性能与资源参数。",
            ),
            3 => (
                "每一个参数，都在掌控之中",
                "缓存、访问控制、任务进阶与扩展参数。留空项沿用 OVMS 默认值。",
            ),
            4 => (
                "查看服务的每一步",
                "实时收集 stdout / stderr，保留最近 1,000 行，可复制或导出。",
            ),
            5 => (
                "内置 OVMS 运行时",
                "运行时随应用提供，自动配置环境。可查看版本与完整启动参数。",
            ),
            _ => ("启动你的模型服务", "选择模型、配置推理，一键开放 API。"),
        };
        ui.set_page_title(title.into());
        ui.set_page_description(description.into());
        self.refresh(ui, true);
    }

    fn append_log(&mut self, stream: &str, text: &str) {
        let t = self.boot.elapsed();
        for line in text.lines().take(80) {
            self.logs.push_back(format!(
                "[{:02}:{:02}.{:01}] [{stream}] {line}",
                t.as_secs() / 60,
                t.as_secs() % 60,
                t.subsec_millis() / 100
            ));
            if self.logs.len() > 1000 {
                self.logs.pop_front();
            }
        }
    }
    fn log_text(&self) -> String {
        self.logs.iter().cloned().collect::<Vec<_>>().join("\n")
    }
    fn notice(&mut self, ui: &MainWindow, text: impl Into<String>) {
        let text = text.into();
        ui.set_toast(text.as_str().into());
        self.append_log("studio", &text);
        ui.set_log_text(self.log_text().into());
    }
    fn persist(&self) -> Result<(), String> {
        if self.persist {
            config::save_json(&self.state_file, &self.library)?;
        }
        Ok(())
    }
    fn start(&mut self, ui: &MainWindow) {
        if ui.get_running() {
            return;
        }
        self.library.current.name = ui.get_profile_name().trim().to_owned();
        match LaunchSpec::prepare(&self.library.current) {
            Ok(spec) => {
                self.notice(ui, format!("启动 {}", spec.profile.value("model_name")));
                self.active = Some(self.library.current.clone());
                ui.set_running(true);
                ui.set_ready(false);
                ui.set_failed(false);
                ui.set_service_state("正在启动".into());
                ui.set_state_detail("检查启动参数，准备加载模型…".into());
                ui.set_health_detail("等待 OVMS REST 健康检查".into());
                if let Err(e) = self.manager.start(spec) {
                    self.active = None;
                    ui.set_running(false);
                    self.notice(ui, e);
                }
                if let Err(e) = self.persist() {
                    self.notice(ui, e);
                }
            }
            Err(e) => {
                ui.set_failed(true);
                ui.set_service_state("配置需调整".into());
                ui.set_state_detail(e.as_str().into());
                self.notice(ui, e);
            }
        }
        self.refresh(ui, false);
    }
    fn poll(&mut self, ui: &MainWindow) {
        let events = self.manager.events.try_iter().collect::<Vec<_>>();
        let mut dirty = false;
        for event in events {
            match event {
                Event::Started(spec) => {
                    self.active = Some(spec.profile);
                    self.launched_at = Some(Instant::now());
                }
                Event::Health { ready, detail } => {
                    ui.set_ready(ready);
                    ui.set_health_detail(detail.into());
                }
                Event::State { phase, detail, pid } => {
                    let running = matches!(
                        phase,
                        Phase::Starting
                            | Phase::Ready
                            | Phase::Unready
                            | Phase::Running
                            | Phase::Stopping
                    );
                    let label = match phase {
                        Phase::Idle => "服务已停止",
                        Phase::Starting => "正在加载",
                        Phase::Ready => "服务已就绪",
                        Phase::Unready => "暂未就绪",
                        Phase::Running => "进程运行中",
                        Phase::Stopping => "正在停止",
                        Phase::Failed => "服务异常",
                        Phase::Exited => "服务已退出",
                    };
                    ui.set_running(running);
                    ui.set_ready(phase == Phase::Ready);
                    ui.set_failed(phase == Phase::Failed);
                    ui.set_service_state(label.into());
                    ui.set_state_detail(detail.as_str().into());
                    if let Some(pid) = pid {
                        ui.set_process_info(
                            format!("PID {pid}  ·  {}", uptime(self.launched_at)).into(),
                        );
                    }
                    if !running {
                        self.active = None;
                        self.launched_at = None;
                        ui.set_process_info("无运行中的进程".into());
                        ui.set_health_detail("服务未运行".into());
                        self.refresh(ui, false);
                    }
                    self.append_log("studio", &detail);
                    dirty = true;
                }
            }
        }
        let logs = self.manager.logs.try_iter().take(150).collect::<Vec<_>>();
        for line in logs {
            self.append_log(line.stream, &line.text);
            dirty = true;
        }
        let dropped = self.manager.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            self.append_log(
                "studio",
                &format!("日志量过大，已跳过 {dropped} 行，服务输出仍持续读取"),
            );
            dirty = true;
        }
        if let Some(started) = self.launched_at {
            let pid = ui
                .get_process_info()
                .split('·')
                .next()
                .unwrap_or("")
                .trim()
                .to_owned();
            ui.set_process_info(
                format!("{pid}  ·  已运行 {}s", started.elapsed().as_secs()).into(),
            );
        }
        if dirty {
            ui.set_log_text(self.log_text().into());
        }
        let tasks = self.task_rx.try_iter().collect::<Vec<_>>();
        for result in tasks {
            match result {
                TaskResult::Picked(key, path) => {
                    ui.set_dialog_busy(false);
                    if let Some(path) = path {
                        if key == "model_path"
                            && self.library.current.value("model_name") == "my-model"
                            && let Some(name) = path.file_stem().and_then(|s| s.to_str())
                        {
                            self.library.current.set("model_name", name);
                        }
                        self.library.current.set(&key, path.to_string_lossy());
                        self.refresh(ui, true);
                    }
                }
                TaskResult::Imported(result) => {
                    ui.set_dialog_busy(false);
                    match result {
                        Ok(Some(profile)) => {
                            self.library.current = profile;
                            self.refresh(ui, true);
                            self.notice(ui, "配置已导入");
                        }
                        Ok(None) => {}
                        Err(e) => self.notice(ui, e),
                    }
                }
                TaskResult::Written(result) => {
                    ui.set_dialog_busy(false);
                    match result {
                        Ok(Some(text)) => self.notice(ui, text),
                        Ok(None) => {}
                        Err(e) => self.notice(ui, e),
                    }
                }
                TaskResult::Inspected(result) => {
                    ui.set_inspecting(false);
                    match result {
                        Ok((version, help)) => {
                            ui.set_runtime_version(version.into());
                            ui.set_help_text(help.into());
                            self.notice(ui, "已读取本机 OVMS 版本与帮助");
                        }
                        Err(e) => {
                            ui.set_runtime_version("运行时检测失败".into());
                            ui.set_help_text(e.as_str().into());
                            self.notice(ui, e);
                        }
                    }
                }
            }
        }
    }
}

fn uptime(start: Option<Instant>) -> String {
    format!(
        "已运行 {}s",
        start.map(|s| s.elapsed().as_secs()).unwrap_or(0)
    )
}
fn api_path(p: &Profile) -> String {
    if p.mode == Mode::Config {
        return "/v2/health/ready".into();
    }
    match p.value("task") {
        "text_generation" => "/v3/chat/completions".into(),
        "embeddings" => "/v3/embeddings".into(),
        "rerank" => "/v3/rerank".into(),
        "image_generation" => "/v3/images/generations".into(),
        "text2speech" => "/v3/audio/speech".into(),
        "speech2text" => "/v3/audio/transcriptions".into(),
        _ => format!("/v2/models/{}/infer", url_segment(p.value("model_name"))),
    }
}
fn url_segment(s: &str) -> String {
    s.as_bytes()
        .iter()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(b) {
                (*b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn example(p: &Profile) -> String {
    let model = p.value("model_name");
    let payload = match p.value("task") {
        "text_generation" => Some(
            serde_json::json!({"model":model,"messages":[{"role":"user","content":"Hello!"}],"max_tokens":128}),
        ),
        "embeddings" => Some(serde_json::json!({"model":model,"input":"Hello from OVMS Studio"})),
        "rerank" => Some(
            serde_json::json!({"model":model,"query":"What is OpenVINO?","documents":["OpenVINO is an inference toolkit."]}),
        ),
        _ => None,
    };
    // GET metadata is valid without knowing tensor names/shapes for an arbitrary model.
    if let Some(payload) = payload {
        format!(
            "curl -X POST \"{}{}\" -H \"Content-Type: application/json\" -d {}",
            p.endpoint(),
            api_path(p),
            if cfg!(windows) {
                format!("\"{}\"", payload.to_string().replace('"', "\\\""))
            } else {
                format!("'{payload}'")
            }
        )
    } else if p.mode == Mode::Config {
        format!("curl \"{}/v2/health/ready\"", p.endpoint())
    } else {
        format!("curl \"{}/v2/models/{}\"", p.endpoint(), url_segment(model))
    }
}

fn wire(ui: &MainWindow, state: &Rc<RefCell<App>>) {
    let a = state.clone();
    ui.on_name_changed(move |name| {
        a.borrow_mut().library.current.name = name.to_string();
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_navigate(move |page| {
        if let Some(ui) = w.upgrade() {
            a.borrow_mut().navigate(&ui, page);
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_field_changed(move |key, value| {
        if let Some(ui) = w.upgrade() {
            if ui.get_running() {
                return;
            }
            let mut app = a.borrow_mut();
            app.library.current.set(
                &key,
                if value == "默认 / 自动" {
                    ""
                } else {
                    &value
                },
            );
            app.refresh(&ui, key == "task");
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_mode_changed(move |mode| {
        if let Some(ui) = w.upgrade() {
            if ui.get_running() {
                return;
            }
            let mut app = a.borrow_mut();
            app.library.current.mode = Mode::from_index(mode);
            app.refresh(&ui, true);
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_filter_changed(move |filter| {
        if let Some(ui) = w.upgrade() {
            let mut app = a.borrow_mut();
            app.filter = filter.to_string();
            app.refresh(&ui, true);
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_start_server(move || {
        if let Some(ui) = w.upgrade() {
            a.borrow_mut().start(&ui);
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_stop_server(move || {
        if let Some(ui) = w.upgrade() {
            ui.set_state_detail("正在停止服务…".into());
            a.borrow().manager.stop();
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_apply_preset(move |preset| {
        if let Some(ui) = w.upgrade() {
            if ui.get_running() {
                return;
            }
            let mut app = a.borrow_mut();
            app.library.current.mode = Mode::Local;
            app.library.current.set(
                "task",
                match preset {
                    0 => "text_generation",
                    1 => "embeddings",
                    _ => "",
                },
            );
            app.navigate(&ui, 0);
            app.notice(&ui, "已应用任务模板，请选择对应模型目录");
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_inspect_runtime(move || {
        if let Some(ui) = w.upgrade() {
            if ui.get_inspecting() {
                return;
            }
            ui.set_inspecting(true);
            let app = a.borrow();
            let profile = app.library.current.clone();
            let tx = app.task_tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(TaskResult::Inspected(runtime::inspect_runtime(&profile)));
            });
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_browse(move |key, folder| {
        if let Some(ui) = w.upgrade() {
            if ui.get_running() || ui.get_dialog_busy() {
                return;
            }
            ui.set_dialog_busy(true);
            let app = a.borrow();
            let tx = app.task_tx.clone();
            let current = app.library.current.value(&key).to_owned();
            std::thread::spawn(move || {
                let key = key.to_string();
                let mut dialog = rfd::FileDialog::new().set_title(if folder {
                    "选择目录"
                } else {
                    "选择文件"
                });
                let p = Path::new(&current);
                if p.is_dir() {
                    dialog = dialog.set_directory(p);
                } else if let Some(p) = p.parent().filter(|p| p.is_dir()) {
                    dialog = dialog.set_directory(p);
                }
                if !folder {
                    dialog = match key.as_str() {
                        "config_path" => dialog.add_filter("OVMS config.json", &["json"]),
                        "model_path" => dialog
                            .add_filter(
                                "模型文件",
                                &[
                                    "onnx",
                                    "xml",
                                    "pb",
                                    "pdmodel",
                                    "pdiparams",
                                    "tflite",
                                    "gguf",
                                ],
                            )
                            .add_filter("所有文件", &["*"]),
                        _ => dialog,
                    };
                }
                let path = if folder {
                    dialog.pick_folder()
                } else {
                    dialog.pick_file()
                };
                let _ = tx.send(TaskResult::Picked(key, path));
            });
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_save_profile(move |name| {
        if let Some(ui) = w.upgrade() {
            let mut app = a.borrow_mut();
            let name = name.trim();
            if name.is_empty() {
                app.notice(&ui, "请先给配置命名");
                return;
            }
            app.library.current.name = name.to_owned();
            let profile = app.library.current.clone();
            app.library.saved.insert(name.into(), profile);
            let result = app.persist();
            app.refresh(&ui, false);
            let persisted = app.persist;
            app.notice(
                &ui,
                match result {
                    Ok(()) if persisted => format!("配置“{name}”已保存"),
                    Ok(()) => format!("配置“{name}”已保存在本次会话，可用“导出”保存为文件"),
                    Err(e) => e,
                },
            );
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_load_profile(move |name| {
        if let Some(ui) = w.upgrade() {
            if ui.get_running() {
                return;
            }
            let mut app = a.borrow_mut();
            if let Some(mut profile) = app.library.saved.get(name.as_str()).cloned() {
                match profile.upgrade() {
                    Ok(()) => {
                        app.library.current = profile;
                        app.refresh(&ui, true);
                        app.notice(&ui, format!("已载入配置：{name}"));
                    }
                    Err(e) => app.notice(&ui, e),
                }
            }
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_import_profile(move || {
        if let Some(ui) = w.upgrade() {
            if ui.get_dialog_busy() || ui.get_running() {
                return;
            }
            ui.set_dialog_busy(true);
            let tx = a.borrow().task_tx.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("导入 OVMS Studio 配置")
                        .add_filter("JSON", &["json"])
                        .pick_file()
                    else {
                        return Ok(None);
                    };
                    load_profile(&path).map(Some)
                })();
                let _ = tx.send(TaskResult::Imported(result));
            });
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_export_profile(move || {
        if let Some(ui) = w.upgrade() {
            if ui.get_dialog_busy() {
                return;
            }
            ui.set_dialog_busy(true);
            let mut profile = a.borrow().library.current.clone();
            profile.name = ui.get_profile_name().to_string();
            let tx = a.borrow().task_tx.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("导出配置")
                        .set_file_name("ovms-profile.json")
                        .add_filter("JSON", &["json"])
                        .save_file()
                    else {
                        return Ok(None);
                    };
                    config::save_json(&path, &profile)?;
                    Ok(Some(format!("配置已导出：{}", path.display())))
                })();
                let _ = tx.send(TaskResult::Written(result));
            });
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_copy_value(move |kind| {
        if let Some(ui) = w.upgrade() {
            let mut app = a.borrow_mut();
            let profile = app.active.as_ref().unwrap_or(&app.library.current);
            let text = match kind.as_str() {
                "command" => profile.preview(),
                "endpoint" => profile.endpoint(),
                "example" => example(profile),
                "logs" => app.log_text(),
                _ => return,
            };
            match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
                Ok(()) => app.notice(&ui, "已复制到剪贴板"),
                Err(e) => app.notice(&ui, format!("复制失败：{e}")),
            }
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_open_url(move |kind| {
        if let Some(ui) = w.upgrade() {
            let mut app = a.borrow_mut();
            let url = match kind.as_str() {
                "docs" => config::DOCS_URL.to_owned(),
                "health" if ui.get_ready() => format!(
                    "{}/v2/health/ready",
                    app.active
                        .as_ref()
                        .unwrap_or(&app.library.current)
                        .endpoint()
                ),
                _ => return,
            };
            if let Err(e) = open::that_detached(url) {
                app.notice(&ui, format!("无法打开浏览器：{e}"));
            }
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_clear_logs(move || {
        if let Some(ui) = w.upgrade() {
            a.borrow_mut().logs.clear();
            ui.set_log_text("".into());
            ui.set_toast("日志显示已清空".into());
        }
    });
    let a = state.clone();
    let w = ui.as_weak();
    ui.on_export_logs(move || {
        if let Some(ui) = w.upgrade() {
            if ui.get_dialog_busy() {
                return;
            }
            ui.set_dialog_busy(true);
            let app = a.borrow();
            let text = app.log_text();
            let tx = app.task_tx.clone();
            std::thread::spawn(move || {
                let result = (|| {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("导出运行日志")
                        .set_file_name("ovms-studio.log")
                        .save_file()
                    else {
                        return Ok(None);
                    };
                    std::fs::write(&path, text).map_err(|e| e.to_string())?;
                    Ok(Some(format!("日志已导出：{}", path.display())))
                })();
                let _ = tx.send(TaskResult::Written(result));
            });
        }
    });
}

fn load_profile(path: &Path) -> Result<Profile, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
        return Err("配置文件超过 2 MB".into());
    }
    let mut profile: Profile =
        serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("配置文件格式不正确：{e}"))?;
    profile.upgrade()?;
    profile.absolutize_paths(path.parent().unwrap_or(Path::new(".")));
    Ok(profile)
}

fn save_snapshot(ui: &MainWindow, path: &Path) -> Result<(), String> {
    let image = ui.window().take_snapshot().map_err(|e| e.to_string())?;
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder =
        png::Encoder::new(std::io::BufWriter::new(file), image.width(), image.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .map_err(|e| e.to_string())?
        .write_image_data(image.as_bytes())
        .map_err(|e| e.to_string())
}

#[derive(Default)]
struct Smoke {
    stage: u8,
    page: i32,
    ready: bool,
    restarted: bool,
    failed: Option<String>,
    last: Option<Instant>,
    screenshots: Vec<String>,
    configuration_callbacks: bool,
}

fn capture_smoke(ui: &MainWindow, output: &Path, report: &mut Smoke, name: &str) {
    match save_snapshot(ui, &output.join(name)) {
        Ok(()) => report.screenshots.push(name.to_owned()),
        Err(e) => {
            report.failed = Some(e);
            let _ = ui.hide();
        }
    }
    report.last = Some(Instant::now());
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let argument = |flag: &str| {
        args.iter()
            .position(|s| s == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let smoke = args.iter().any(|s| s == "--smoke-test");
    let no_persist = smoke || args.iter().any(|s| s == "--no-persist");
    let state_file = config::state_path();
    let mut recovery = None;
    let mut library = if no_persist {
        Library::default()
    } else if state_file.is_file() {
        match std::fs::read(&state_file)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_slice::<Library>(&s).map_err(|e| e.to_string()))
        {
            Ok(mut lib) => {
                lib.current.upgrade()?;
                lib
            }
            Err(e) => {
                recovery = Some(e);
                Library::default()
            }
        }
    } else {
        Library::default()
    };
    if let Some(profile) = argument("--profile") {
        library.current = load_profile(&PathBuf::from(profile))?;
    }
    let ui = MainWindow::new().map_err(|e| e.to_string())?;
    ui.set_application_version(env!("CARGO_PKG_VERSION").into());
    let (task_tx, task_rx) = mpsc::channel();
    let state = Rc::new(RefCell::new(App {
        library,
        manager: Manager::new(),
        active: None,
        launched_at: None,
        logs: VecDeque::new(),
        boot: Instant::now(),
        filter: String::new(),
        task_tx,
        task_rx,
        persist: !no_persist && recovery.is_none(),
        state_file,
    }));
    wire(&ui, &state);
    state.borrow_mut().navigate(&ui, 0);
    state.borrow_mut().notice(
        &ui,
        if let Some(e) = recovery {
            format!("上次配置读取失败，原文件已保留：{e}")
        } else {
            "OVMS 已随应用内置，选择模型后即可启动。".into()
        },
    );
    ui.invoke_inspect_runtime();
    let poll = slint::Timer::default();
    let a = state.clone();
    let w = ui.as_weak();
    poll.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(100),
        move || {
            if let Some(ui) = w.upgrade() {
                a.borrow_mut().poll(&ui);
            }
        },
    );
    let report = Rc::new(RefCell::new(Smoke::default()));
    let smoke_timer = slint::Timer::default();
    if smoke {
        let output =
            PathBuf::from(argument("--capture-dir").unwrap_or_else(|| "work/ui-smoke".into()));
        std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
        let w = ui.as_weak();
        let result = report.clone();
        let deadline = Instant::now() + Duration::from_secs(70);
        smoke_timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(500),
            move || {
                let Some(ui) = w.upgrade() else {
                    return;
                };
                let mut r = result.borrow_mut();
                if Instant::now() > deadline || ui.get_failed() {
                    r.failed = Some(ui.get_state_detail().to_string());
                    let _ = ui.hide();
                    return;
                }
                match r.stage {
                    0 => {
                        ui.invoke_inspect_runtime();
                        ui.invoke_start_server();
                        r.stage = 1;
                    }
                    1 if ui.get_ready() => {
                        r.ready = true;
                        r.stage = 2;
                        r.last = Some(Instant::now());
                    }
                    2 if r
                        .last
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(600)) =>
                    {
                        let name = format!("page-{}.png", r.page);
                        match save_snapshot(&ui, &output.join(&name)) {
                            Ok(()) => r.screenshots.push(name),
                            Err(e) => {
                                r.failed = Some(e);
                                let _ = ui.hide();
                                return;
                            }
                        }
                        if r.page < 5 {
                            r.page += 1;
                            ui.invoke_navigate(r.page);
                            r.last = Some(Instant::now());
                        } else {
                            ui.invoke_stop_server();
                            r.stage = 3;
                        }
                    }
                    3 if !ui.get_running() => {
                        ui.invoke_navigate(0);
                        ui.invoke_save_profile(ui.get_profile_name());
                        ui.invoke_mode_changed(1);
                        r.last = Some(Instant::now());
                        r.stage = 4;
                    }
                    4 if r
                        .last
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(600)) =>
                    {
                        capture_smoke(&ui, &output, &mut r, "hugging-face.png");
                        ui.invoke_mode_changed(2);
                        r.stage = 5;
                    }
                    5 if r
                        .last
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(600)) =>
                    {
                        capture_smoke(&ui, &output, &mut r, "multi-model.png");
                        ui.invoke_apply_preset(1);
                        ui.invoke_navigate(2);
                        r.configuration_callbacks = ui.get_task_label() == "embeddings";
                        r.stage = 6;
                    }
                    6 if r
                        .last
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(600)) =>
                    {
                        capture_smoke(&ui, &output, &mut r, "embeddings.png");
                        ui.invoke_apply_preset(0);
                        ui.invoke_navigate(2);
                        r.configuration_callbacks &= ui.get_task_label() == "text_generation";
                        r.stage = 7;
                    }
                    7 if r
                        .last
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(600)) =>
                    {
                        capture_smoke(&ui, &output, &mut r, "text-generation.png");
                        ui.invoke_load_profile(ui.get_profile_name());
                        ui.invoke_navigate(0);
                        r.stage = 8;
                    }
                    8 if r
                        .last
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(600)) =>
                    {
                        capture_smoke(&ui, &output, &mut r, "stopped.png");
                        ui.invoke_start_server();
                        r.stage = 9;
                    }
                    9 if ui.get_ready() => {
                        r.restarted = true;
                        let _ = save_snapshot(&ui, &output.join("restarted.png"));
                        r.stage = 10;
                        let _ = ui.hide();
                    }
                    _ => {}
                }
            },
        );
    }
    ui.run().map_err(|e| e.to_string())?;
    poll.stop();
    smoke_timer.stop();
    drop(poll);
    drop(smoke_timer);
    let mut app = state.borrow_mut();
    app.library.current.name = ui.get_profile_name().to_string();
    let last_profile = app.library.current.clone();
    let persisted = app.persist();
    drop(app);
    drop(ui);
    drop(state);
    persisted?;
    if smoke {
        let r = report.borrow();
        let stopped = !matches!(
            runtime::http_ready(
                config::connect_host(last_profile.value("rest_bind_address")),
                last_profile.port("rest_port")?
            ),
            Ok(true)
        );
        let output =
            PathBuf::from(argument("--capture-dir").unwrap_or_else(|| "work/ui-smoke".into()));
        config::save_json(
            &output.join("report.json"),
            &serde_json::json!({"ready":r.ready,"restart_ready":r.restarted,"stopped_on_close":stopped,"configuration_callbacks":r.configuration_callbacks,"error":r.failed,"screenshots":r.screenshots}),
        )?;
        if !r.ready || !r.restarted || !stopped || !r.configuration_callbacks || r.failed.is_some()
        {
            return Err(format!("UI smoke test failed: {:?}", r.failed));
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        if std::env::args().any(|s| s == "--smoke-test") {
            std::process::exit(1);
        } else {
            rfd::MessageDialog::new()
                .set_title("OVMS Studio")
                .set_description(&e)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
    }
}
