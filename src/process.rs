use crate::runtime::LaunchSpec;
use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, Sender, SyncSender},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Starting,
    Ready,
    Unready,
    Running,
    Stopping,
    Failed,
    Exited,
}

#[derive(Clone, Debug)]
pub enum Event {
    State {
        phase: Phase,
        detail: String,
        pid: Option<u32>,
    },
    Started(Box<LaunchSpec>),
    Health {
        ready: bool,
        detail: String,
    },
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub stream: &'static str,
    pub text: String,
}

enum Control {
    Start(Box<LaunchSpec>),
    Stop,
    Shutdown,
}

pub struct Manager {
    control: Sender<Control>,
    worker: Option<JoinHandle<()>>,
    pub events: Receiver<Event>,
    pub logs: Receiver<LogLine>,
    pub dropped: Arc<AtomicUsize>,
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
}

impl Manager {
    pub fn new() -> Self {
        let (control, rx) = mpsc::channel();
        let (events, erx) = mpsc::channel();
        let (logs, lrx) = mpsc::sync_channel(1024);
        let dropped = Arc::new(AtomicUsize::new(0));
        let counter = dropped.clone();
        let worker = thread::spawn(move || supervise(rx, events, logs, counter));
        Self {
            control,
            worker: Some(worker),
            events: erx,
            logs: lrx,
            dropped,
        }
    }
    pub fn start(&self, spec: LaunchSpec) -> Result<(), String> {
        self.control
            .send(Control::Start(Box::new(spec)))
            .map_err(|e| e.to_string())
    }
    pub fn stop(&self) {
        let _ = self.control.send(Control::Stop);
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Active {
    child: Child,
    spec: LaunchSpec,
    ready: bool,
    last_probe: Instant,
    readers: Vec<JoinHandle<()>>,
    #[cfg(windows)]
    job: Job,
}

fn state(tx: &Sender<Event>, phase: Phase, detail: impl Into<String>, pid: Option<u32>) {
    let _ = tx.send(Event::State {
        phase,
        detail: detail.into(),
        pid,
    });
}

fn supervise(
    rx: Receiver<Control>,
    events: Sender<Event>,
    logs: SyncSender<LogLine>,
    dropped: Arc<AtomicUsize>,
) {
    let mut active: Option<Active> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(80)) {
            Ok(Control::Start(spec)) => {
                if active.is_some() {
                    continue;
                }
                state(&events, Phase::Starting, "检查模型路径与监听端口…", None);
                if let Err(e) = spec.check_ports() {
                    state(&events, Phase::Failed, e, None);
                    continue;
                }
                match spec.command().spawn() {
                    Ok(mut child) => {
                        let pid = child.id();
                        #[cfg(windows)]
                        let job = match Job::assign(&child) {
                            Ok(job) => job,
                            Err(e) => {
                                let _ = child.kill();
                                let _ = child.wait();
                                state(&events, Phase::Failed, e, None);
                                continue;
                            }
                        };
                        let mut readers = vec![];
                        if let Some(pipe) = child.stdout.take() {
                            readers.push(read_pipe(pipe, "stdout", logs.clone(), dropped.clone()));
                        }
                        if let Some(pipe) = child.stderr.take() {
                            readers.push(read_pipe(pipe, "stderr", logs.clone(), dropped.clone()));
                        }
                        let _ = events.send(Event::Started(spec.clone()));
                        state(
                            &events,
                            Phase::Starting,
                            "进程已启动，等待模型加载与 REST 就绪…",
                            Some(pid),
                        );
                        active = Some(Active {
                            child,
                            spec: *spec,
                            ready: false,
                            last_probe: Instant::now() - Duration::from_secs(2),
                            readers,
                            #[cfg(windows)]
                            job,
                        });
                    }
                    Err(e) => state(
                        &events,
                        Phase::Failed,
                        format!("启动失败：{e}。请检查 OVMS 的 DLL / 运行库与执行权限。"),
                        None,
                    ),
                }
            }
            Ok(Control::Stop) => {
                if let Some(mut process) = active.take() {
                    state(
                        &events,
                        Phase::Stopping,
                        "正在终止 OVMS 及其子进程…",
                        Some(process.child.id()),
                    );
                    terminate(&mut process);
                    for reader in process.readers {
                        let _ = reader.join();
                    }
                    state(&events, Phase::Idle, "服务已停止，端口已释放", None);
                }
            }
            Ok(Control::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(mut process) = active.take() {
                    terminate(&mut process);
                    for reader in process.readers {
                        let _ = reader.join();
                    }
                }
                return;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let Some(process) = active.as_mut() else {
            continue;
        };
        match process.child.try_wait() {
            Ok(Some(exit)) => {
                let process = active.take().unwrap();
                // Dropping the Job also terminates lingering conversion processes.
                #[cfg(windows)]
                drop(process.job);
                for reader in process.readers {
                    let _ = reader.join();
                }
                let phase = if exit.success() {
                    Phase::Exited
                } else {
                    Phase::Failed
                };
                state(
                    &events,
                    phase,
                    format!("OVMS 已退出（{exit}），请查看运行日志"),
                    None,
                );
                continue;
            }
            Err(e) => {
                let mut process = active.take().unwrap();
                terminate(&mut process);
                state(
                    &events,
                    Phase::Failed,
                    format!("无法读取进程状态：{e}"),
                    None,
                );
                continue;
            }
            _ => {}
        }
        if process.last_probe.elapsed() >= Duration::from_secs(1) {
            process.last_probe = Instant::now();
            if process.spec.profile.port("rest_port").unwrap_or(0) == 0 {
                if !process.ready {
                    state(
                        &events,
                        Phase::Running,
                        "仅启用 gRPC：进程运行中，未验证模型就绪",
                        Some(process.child.id()),
                    );
                    process.ready = true;
                }
                continue;
            }
            let result = process.spec.readiness();
            let ready = matches!(result, Ok(true));
            let detail = match result {
                Ok(true) => "GET /v2/health/ready → HTTP 200".into(),
                Ok(false) => "REST 可连接，模型尚未就绪".into(),
                Err(e) => format!("等待 REST 就绪：{e}"),
            };
            let _ = events.send(Event::Health {
                ready,
                detail: detail.clone(),
            });
            if ready != process.ready {
                state(
                    &events,
                    if ready { Phase::Ready } else { Phase::Unready },
                    if ready {
                        "服务已就绪，可调用 API"
                    } else {
                        "进程运行中，REST 健康检查未通过"
                    },
                    Some(process.child.id()),
                );
                process.ready = ready;
            }
        }
    }
}

fn read_pipe(
    pipe: impl Read + Send + 'static,
    stream: &'static str,
    tx: SyncSender<LogLine>,
    dropped: Arc<AtomicUsize>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        loop {
            let mut bytes = Vec::new();
            let result = reader
                .by_ref()
                .take(16 * 1024)
                .read_until(b'\n', &mut bytes);
            match result {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let text = String::from_utf8_lossy(&bytes)
                        .trim_end_matches(['\r', '\n'])
                        .to_owned();
                    if tx.try_send(LogLine { stream, text }).is_err() {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    })
}

fn terminate(process: &mut Active) {
    #[cfg(windows)]
    process.job.terminate();
    let _ = process.child.kill();
    let _ = process.child.wait();
}

#[cfg(windows)]
struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Job {
    fn assign(child: &Child) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(format!(
                "无法创建进程组：{}",
                std::io::Error::last_os_error()
            ));
        }
        let job = Self(handle);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                std::mem::size_of_val(&info) as u32,
            )
        };
        if ok == 0 {
            return Err(format!(
                "无法设置进程组：{}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } == 0 {
            return Err(format!(
                "无法管理 OVMS 进程组：{}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(job)
    }
    fn terminate(&self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}
#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
