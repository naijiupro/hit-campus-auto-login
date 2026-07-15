use std::{
    ffi::OsStr,
    fs,
    io::{self, Write},
    path::Path,
    process::{Command as StdCommand, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures_util::StreamExt;
use gtk::{glib, prelude::*};
use hit_auto_login_core::{Configuration, CoreError, RunTrigger, WifiAdapter};
use ksni::TrayMethods;
use tokio::{process::Command, time::timeout};

use crate::{
    app::{AppController, AppError, AppEvent, StatusLevel, StatusUpdate},
    config_store::{self, SingleInstance},
};

#[derive(Default)]
pub struct PlatformWifi;

#[async_trait]
impl WifiAdapter for PlatformWifi {
    async fn ensure_connected(&self, ssid: &str) -> Result<(), CoreError> {
        run_nmcli(
            &["--wait", "5", "radio", "wifi", "on"],
            Duration::from_secs(8),
        )
        .await?;
        run_nmcli(
            &["--wait", "15", "device", "wifi", "connect", ssid],
            Duration::from_secs(18),
        )
        .await
    }
}

async fn run_nmcli(arguments: &[&str], limit: Duration) -> Result<(), CoreError> {
    let mut command = Command::new("nmcli");
    command
        .args(arguments)
        .env("LC_ALL", "C")
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = timeout(limit, command.output())
        .await
        .map_err(|_| CoreError::Wifi("NetworkManager 操作超时".into()))?
        .map_err(|_| CoreError::Wifi("无法执行 nmcli，请确认已安装 NetworkManager".into()))?;
    if output.status.success() {
        return Ok(());
    }
    let message = if output.stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    Err(CoreError::Wifi(sanitize_command_message(&message)))
}

fn sanitize_command_message(message: &str) -> String {
    let collapsed = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return "NetworkManager 返回失败状态".into();
    }
    collapsed.chars().take(180).collect()
}

pub fn set_launch_at_login(enabled: bool) -> io::Result<()> {
    let base = directories::BaseDirs::new().ok_or_else(|| io::Error::other("无法确定用户目录"))?;
    let unit_dir = base.config_dir().join("systemd/user");
    let unit_path = unit_dir.join("hit-auto-login.service");
    fs::create_dir_all(&unit_dir)?;

    if enabled {
        let executable = std::env::current_exe()?;
        let exec = systemd_quote(&executable);
        let unit = format!(
            "[Unit]\nDescription=HIT Campus Network Auto Login\nAfter=graphical-session.target NetworkManager.service\nPartOf=graphical-session.target\n\n[Service]\nType=simple\nExecStart={exec} --autostart\nRestart=no\n\n[Install]\nWantedBy=default.target\n"
        );
        fs::write(&unit_path, unit)?;
    }

    run_systemctl(["--user", "daemon-reload"])?;
    if enabled {
        run_systemctl(["--user", "enable", "hit-auto-login.service"])
    } else {
        let result = run_systemctl(["--user", "disable", "hit-auto-login.service"]);
        if unit_path.exists() {
            let _ = fs::remove_file(unit_path);
            let _ = run_systemctl(["--user", "daemon-reload"]);
        }
        result
    }
}

fn systemd_quote(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%");
    format!("\"{escaped}\"")
}

fn run_systemctl<I, S>(arguments: I) -> io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = StdCommand::new("systemctl")
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| io::Error::other("无法执行 systemctl --user"))?;
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(io::Error::other("systemctl --user 返回失败状态"))
            };
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "systemctl --user 操作超时",
            ));
        }
        thread::sleep(Duration::from_millis(80));
    }
}

#[zbus::proxy(
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1",
    interface = "org.freedesktop.login1.Manager"
)]
trait LoginManager {
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;
}

#[zbus::proxy(
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver",
    interface = "org.freedesktop.ScreenSaver"
)]
trait ScreenSaver {
    #[zbus(signal)]
    fn active_changed(&self, active: bool) -> zbus::Result<()>;
}

async fn listen_for_resume(controller: Arc<AppController>) -> zbus::Result<()> {
    let connection = zbus::Connection::system().await?;
    let proxy = LoginManagerProxy::new(&connection).await?;
    let mut signals = proxy.receive_prepare_for_sleep().await?;
    while let Some(signal) = signals.next().await {
        if let Ok(args) = signal.args() {
            if !args.start {
                controller.trigger(RunTrigger::Resume);
            }
        }
    }
    Ok(())
}

async fn listen_for_screen_resume(controller: Arc<AppController>) -> zbus::Result<()> {
    let connection = zbus::Connection::session().await?;
    let proxy = ScreenSaverProxy::new(&connection).await?;
    let mut signals = proxy.receive_active_changed().await?;
    while let Some(signal) = signals.next().await {
        if let Ok(args) = signal.args() {
            if !args.active {
                controller.trigger(RunTrigger::ScreenResume);
            }
        }
    }
    Ok(())
}

enum UiCommand {
    Show,
    Status(StatusUpdate),
    Quit,
}

#[derive(Debug)]
struct LinuxTray {
    ui: glib::Sender<UiCommand>,
    controller: Arc<AppController>,
}

impl ksni::Tray for LinuxTray {
    fn id(&self) -> String {
        "hit-auto-login".into()
    }

    fn title(&self) -> String {
        "HIT 校园网自动登录".into()
    }

    fn icon_name(&self) -> String {
        "network-wireless".into()
    }

    fn activate(&mut self, _: i32, _: i32) {
        let _ = self.ui.send(UiCommand::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{MenuItem, StandardItem};
        vec![
            StandardItem {
                label: "打开设置".into(),
                icon_name: "preferences-system".into(),
                activate: Box::new(|tray| {
                    let _ = tray.ui.send(UiCommand::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "立即检测".into(),
                icon_name: "network-wireless".into(),
                activate: Box::new(|tray| tray.controller.trigger(RunTrigger::Manual)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "退出".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|tray| {
                    let _ = tray.ui.send(UiCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

struct LinuxUi {
    window: gtk::Window,
    username: gtk::Entry,
    password: gtk::Entry,
    launch_at_login: gtk::CheckButton,
    save_detect: gtk::Button,
    status: gtk::Label,
    detail: gtk::Label,
    controller: Arc<AppController>,
}

impl LinuxUi {
    fn build(controller: Arc<AppController>) -> Self {
        let configuration = controller.configuration();
        let window = gtk::Window::new(gtk::WindowType::Toplevel);
        window.set_title("HIT 校园网自动登录");
        window.set_default_size(430, 380);
        window.set_border_width(16);
        window.set_resizable(false);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 10);
        let title = gtk::Label::new(Some("HIT 校园网自动登录"));
        title.set_xalign(0.0);
        let subtitle = gtk::Label::new(Some("HIT-WLAN · wp.hit.edu.cn"));
        subtitle.set_xalign(0.0);
        let username = gtk::Entry::new();
        username.set_placeholder_text(Some("学号"));
        username.set_text(&configuration.username);
        let password = gtk::Entry::new();
        password.set_placeholder_text(Some("密码"));
        password.set_visibility(false);
        password.set_text(&configuration.password);
        let privacy = gtk::Label::new(Some(
            "账号密码以明文配置文件保存；日志不会记录密码、challenge 或完整认证参数。",
        ));
        privacy.set_xalign(0.0);
        privacy.set_line_wrap(true);
        let launch_at_login = gtk::CheckButton::with_label("登录 Linux 桌面时自动启动");
        launch_at_login.set_active(configuration.launch_at_login);
        let button_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let save = gtk::Button::with_label("保存");
        let save_detect = gtk::Button::with_label("保存并立即检测");
        let exit = gtk::Button::with_label("退出");
        button_row.pack_start(&save, false, false, 0);
        button_row.pack_start(&save_detect, false, false, 0);
        button_row.pack_end(&exit, false, false, 0);
        let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
        let status = gtk::Label::new(Some("等待首次检测"));
        status.set_xalign(0.0);
        let detail = gtk::Label::new(Some("事件触发运行，无后台网络轮询"));
        detail.set_xalign(0.0);
        detail.set_line_wrap(true);

        for widget in [
            title.upcast_ref::<gtk::Widget>(),
            subtitle.upcast_ref(),
            username.upcast_ref(),
            password.upcast_ref(),
            privacy.upcast_ref(),
            launch_at_login.upcast_ref(),
            button_row.upcast_ref(),
            separator.upcast_ref(),
            status.upcast_ref(),
            detail.upcast_ref(),
        ] {
            root.pack_start(widget, false, false, 0);
        }
        window.add(&root);

        let ui = Self {
            window,
            username,
            password,
            launch_at_login,
            save_detect,
            status,
            detail,
            controller,
        };

        let save_ui = ui.clone_handles();
        save.connect_clicked(move |_| save_ui.save(false));
        let detect_ui = ui.clone_handles();
        ui.save_detect
            .connect_clicked(move |_| detect_ui.save(true));
        exit.connect_clicked(|_| gtk::main_quit());
        ui.window.connect_delete_event(|window, _| {
            window.hide();
            gtk::Inhibit(true)
        });
        ui
    }

    fn clone_handles(&self) -> LinuxUiHandles {
        LinuxUiHandles {
            window: self.window.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            launch_at_login: self.launch_at_login.clone(),
            controller: self.controller.clone(),
        }
    }

    fn apply_status(&self, update: StatusUpdate) {
        self.status.set_text(&update.message);
        self.detail.set_text(&update.detail);
        self.save_detect.set_sensitive(!update.running);
        if matches!(update.level, StatusLevel::Failure) {
            self.window.set_urgency_hint(true);
        }
    }
}

struct LinuxUiHandles {
    window: gtk::Window,
    username: gtk::Entry,
    password: gtk::Entry,
    launch_at_login: gtk::CheckButton,
    controller: Arc<AppController>,
}

impl LinuxUiHandles {
    fn save(&self, detect: bool) {
        let mut configuration = self.controller.configuration();
        configuration.username = self.username.text().to_string();
        configuration.password = self.password.text().to_string();
        configuration.launch_at_login = self.launch_at_login.is_active();
        match self.controller.save(configuration) {
            Ok(()) if detect => self.controller.trigger(RunTrigger::Manual),
            Ok(()) => {}
            Err(error) => {
                let dialog = gtk::MessageDialog::new(
                    Some(&self.window),
                    gtk::DialogFlags::MODAL,
                    gtk::MessageType::Error,
                    gtk::ButtonsType::Close,
                    &error.to_string(),
                );
                dialog.run();
                dialog.close();
            }
        }
    }
}

pub fn run() -> Result<(), AppError> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument == "--configure") {
        return configure_interactively();
    }
    if arguments.iter().any(|argument| argument == "--check-once") {
        return check_once();
    }
    run_tray()
}

fn run_tray() -> Result<(), AppError> {
    let _instance = SingleInstance::acquire()
        .map_err(|error| AppError::Platform(error.to_string()))?
        .ok_or(AppError::AlreadyRunning)?;
    gtk::init().map_err(|error| AppError::Platform(error.to_string()))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::Platform(error.to_string()))?;
    let (event_sender, event_receiver) = mpsc::channel();
    let controller = Arc::new(AppController::new(runtime.handle().clone(), event_sender)?);
    let ui = RcUi::new(LinuxUi::build(controller.clone()));

    let (ui_sender, ui_receiver) = glib::MainContext::channel(glib::Priority::default());
    let event_ui_sender = ui_sender.clone();
    thread::spawn(move || {
        while let Ok(event) = event_receiver.recv() {
            let AppEvent::Status(status) = event;
            if event_ui_sender.send(UiCommand::Status(status)).is_err() {
                break;
            }
        }
    });

    let receiver_ui = ui.clone();
    ui_receiver.attach(None, move |command| {
        match command {
            UiCommand::Show => {
                receiver_ui.0.window.show_all();
                receiver_ui.0.window.present();
            }
            UiCommand::Status(status) => receiver_ui.0.apply_status(status),
            UiCommand::Quit => {
                gtk::main_quit();
                return glib::ControlFlow::Break;
            }
        }
        glib::ControlFlow::Continue
    });

    let tray = LinuxTray {
        ui: ui_sender,
        controller: controller.clone(),
    };
    runtime.spawn(async move {
        if let Ok(_handle) = tray.spawn().await {
            std::future::pending::<()>().await;
        }
    });
    runtime.spawn(listen_for_resume(controller.clone()));
    runtime.spawn(listen_for_screen_resume(controller.clone()));
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        controller.trigger(RunTrigger::Launch);
    });

    ui.0.window.show_all();
    if ui.0.controller.configuration().credentials_present() {
        ui.0.window.hide();
    }
    gtk::main();
    Ok(())
}

#[derive(Clone)]
struct RcUi(std::rc::Rc<LinuxUi>);

impl RcUi {
    fn new(ui: LinuxUi) -> Self {
        Self(std::rc::Rc::new(ui))
    }
}

fn configure_interactively() -> Result<(), AppError> {
    let mut configuration =
        config_store::load().map_err(|error| AppError::Config(error.to_string()))?;
    println!("HIT 校园网自动登录 - Linux 交互配置");
    println!("配置文件将以 0600 权限明文保存账号密码。程序不会在日志中输出认证敏感参数。\n");

    print!(
        "学号{}: ",
        if configuration.username.is_empty() {
            ""
        } else {
            "（回车保留当前值）"
        }
    );
    io::stdout()
        .flush()
        .map_err(|error| AppError::Config(error.to_string()))?;
    let username = read_line()?;
    if !username.trim().is_empty() {
        configuration.username = username.trim().to_owned();
    }

    let password = rpassword::prompt_password(if configuration.password.is_empty() {
        "密码: "
    } else {
        "密码（回车保留当前值）: "
    })
    .map_err(|error| AppError::Config(error.to_string()))?;
    if !password.is_empty() {
        configuration.password = password;
    }

    configuration.launch_at_login =
        prompt_yes_no("登录桌面时自动启动", configuration.launch_at_login)?;
    configuration.normalize();
    if !configuration.credentials_present() {
        return Err(AppError::Config("学号和密码不能为空".into()));
    }
    config_store::save(&configuration).map_err(|error| AppError::Config(error.to_string()))?;
    set_launch_at_login(configuration.launch_at_login)
        .map_err(|error| AppError::Platform(error.to_string()))?;
    println!(
        "配置已保存：{}",
        config_store::config_path()
            .map_err(|error| AppError::Config(error.to_string()))?
            .display()
    );

    if prompt_yes_no("现在执行一次联网检测", true)? {
        check_once()?;
    }
    Ok(())
}

fn check_once() -> Result<(), AppError> {
    let _instance = SingleInstance::acquire()
        .map_err(|error| AppError::Platform(error.to_string()))?
        .ok_or(AppError::AlreadyRunning)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::Platform(error.to_string()))?;
    let (sender, receiver) = mpsc::channel();
    let controller = Arc::new(AppController::new(runtime.handle().clone(), sender)?);
    controller.trigger(RunTrigger::Manual);
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut started = false;
    while Instant::now() < deadline {
        let event = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| AppError::Platform(error.to_string()))?;
        let AppEvent::Status(status) = event;
        println!("{}", status.message);
        started |= status.running;
        if started && !status.running {
            return if status.level == StatusLevel::Success {
                Ok(())
            } else {
                Err(AppError::Platform(status.message))
            };
        }
    }
    Err(AppError::Platform("单次检测在 90 秒内未结束".into()))
}

fn prompt_yes_no(prompt: &str, default: bool) -> Result<bool, AppError> {
    loop {
        print!("{prompt} [{}]: ", if default { "Y/n" } else { "y/N" });
        io::stdout()
            .flush()
            .map_err(|error| AppError::Config(error.to_string()))?;
        let value = read_line()?.trim().to_ascii_lowercase();
        match value.as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => println!("请输入 y 或 n。"),
        }
    }
}

fn read_line() -> Result<String, AppError> {
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|error| AppError::Config(error.to_string()))?;
    Ok(value)
}

pub fn show_fatal_error(message: &str) {
    eprintln!("HIT 校园网自动登录：{message}");
}
