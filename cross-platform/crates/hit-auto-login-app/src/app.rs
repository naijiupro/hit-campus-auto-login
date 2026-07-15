use std::sync::{Arc, Mutex, mpsc::Sender};

use hit_auto_login_core::{
    Configuration, RunTrigger, SrunPortalClient, SystemConnectivityChecker, Workflow,
    WorkflowCoordinator,
};
use thiserror::Error;
use tokio::runtime::Handle;

use crate::{
    config_store,
    platform::{PlatformWifi, set_launch_at_login},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusLevel {
    Idle,
    Working,
    Success,
    Warning,
    Failure,
}

#[derive(Clone, Debug)]
pub struct StatusUpdate {
    pub level: StatusLevel,
    pub message: String,
    pub detail: String,
    pub running: bool,
}

#[derive(Clone, Debug)]
pub enum AppEvent {
    Status(StatusUpdate),
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("配置错误：{0}")]
    Config(String),
    #[error("系统集成错误：{0}")]
    Platform(String),
    #[error("程序已在运行")]
    AlreadyRunning,
}

pub struct AppController {
    configuration: Arc<Mutex<Configuration>>,
    coordinator: Arc<WorkflowCoordinator>,
    runtime: Handle,
    events: Sender<AppEvent>,
}

impl AppController {
    pub fn new(runtime: Handle, events: Sender<AppEvent>) -> Result<Self, AppError> {
        let configuration =
            config_store::load().map_err(|error| AppError::Config(error.to_string()))?;
        Ok(Self {
            configuration: Arc::new(Mutex::new(configuration)),
            coordinator: Arc::new(WorkflowCoordinator::default()),
            runtime,
            events,
        })
    }

    pub fn configuration(&self) -> Configuration {
        self.configuration
            .lock()
            .expect("configuration mutex poisoned")
            .clone()
    }

    pub fn save(&self, mut configuration: Configuration) -> Result<(), AppError> {
        configuration.normalize();
        config_store::save(&configuration).map_err(|error| AppError::Config(error.to_string()))?;
        set_launch_at_login(configuration.launch_at_login)
            .map_err(|error| AppError::Platform(error.to_string()))?;
        *self
            .configuration
            .lock()
            .expect("configuration mutex poisoned") = configuration;
        self.send_status(StatusUpdate {
            level: StatusLevel::Idle,
            message: "设置已保存".into(),
            detail: "账号密码按需求以明文配置保存；状态信息不会记录认证敏感参数".into(),
            running: false,
        });
        Ok(())
    }

    pub fn trigger(&self, trigger: RunTrigger) {
        let configuration = self.configuration.clone();
        let coordinator = self.coordinator.clone();
        let events = self.events.clone();
        self.runtime.spawn(async move {
            let Some(_guard) = coordinator.try_begin(trigger) else {
                return;
            };
            let config = configuration
                .lock()
                .expect("configuration mutex poisoned")
                .clone();
            if !config.credentials_present() {
                let _ = events.send(AppEvent::Status(StatusUpdate {
                    level: StatusLevel::Warning,
                    message: "请先填写学号和密码".into(),
                    detail: "打开设置后点击“保存并立即检测”".into(),
                    running: false,
                }));
                return;
            }

            let _ = events.send(AppEvent::Status(StatusUpdate {
                level: StatusLevel::Working,
                message: "开始检测".into(),
                detail: format!("触发原因：{}", trigger.label()),
                running: true,
            }));

            let connectivity = match SystemConnectivityChecker::new() {
                Ok(value) => value,
                Err(error) => {
                    send_failure(&events, trigger, error.to_string());
                    return;
                }
            };
            let portal = match SrunPortalClient::new(platform_os(), platform_name()) {
                Ok(value) => value,
                Err(error) => {
                    send_failure(&events, trigger, error.to_string());
                    return;
                }
            };
            let workflow = Workflow::new(connectivity, PlatformWifi::default(), portal);
            let progress_events = events.clone();
            let progress = Arc::new(move |message: &str| {
                let _ = progress_events.send(AppEvent::Status(StatusUpdate {
                    level: StatusLevel::Working,
                    message: message.to_owned(),
                    detail: format!("触发原因：{}", trigger.label()),
                    running: true,
                }));
            });

            match workflow.execute(&config, progress).await {
                Ok(result) => {
                    let _ = events.send(AppEvent::Status(StatusUpdate {
                        level: StatusLevel::Success,
                        message: result.message,
                        detail: format!("{} · 已完成", trigger.label()),
                        running: false,
                    }));
                }
                Err(error) => send_failure(&events, trigger, error.to_string()),
            }
        });
    }

    fn send_status(&self, status: StatusUpdate) {
        let _ = self.events.send(AppEvent::Status(status));
    }
}

fn send_failure(events: &Sender<AppEvent>, trigger: RunTrigger, message: String) {
    let _ = events.send(AppEvent::Status(StatusUpdate {
        level: StatusLevel::Failure,
        message,
        detail: format!("{} · 已停止", trigger.label()),
        running: false,
    }));
}

#[cfg(target_os = "windows")]
const fn platform_os() -> &'static str {
    "Windows"
}
#[cfg(target_os = "windows")]
const fn platform_name() -> &'static str {
    "Windows PC"
}
#[cfg(target_os = "linux")]
const fn platform_os() -> &'static str {
    "Linux"
}
#[cfg(target_os = "linux")]
const fn platform_name() -> &'static str {
    "Linux PC"
}
