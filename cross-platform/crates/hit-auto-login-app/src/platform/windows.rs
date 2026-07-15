use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::c_void,
    io,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use hit_auto_login_core::{CoreError, RunTrigger, WifiAdapter};
use native_windows_gui as nwg;
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE},
        NetworkManagement::WiFi::{
            WLAN_CONNECTION_ATTRIBUTES, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO,
            WLAN_INTERFACE_INFO_LIST, WlanCloseHandle, WlanConnect, WlanEnumInterfaces,
            WlanFreeMemory, WlanOpenHandle, WlanQueryInterface, WlanSetProfile,
            dot11_BSS_type_infrastructure, wlan_connection_mode_profile,
            wlan_interface_state_connected, wlan_intf_opcode_current_connection,
        },
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
            RegCreateKeyExW, RegDeleteValueW, RegSetValueExW,
        },
    },
    core::{GUID, PCWSTR, w},
};

use crate::{
    app::{AppController, AppError, AppEvent, StatusLevel, StatusUpdate},
    config_store::SingleInstance,
};

const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

#[derive(Default)]
pub struct PlatformWifi;

#[async_trait]
impl WifiAdapter for PlatformWifi {
    async fn ensure_connected(&self, ssid: &str) -> Result<(), CoreError> {
        let ssid = ssid.to_owned();
        tokio::task::spawn_blocking(move || connect_wifi(&ssid))
            .await
            .map_err(|_| CoreError::Wifi("Windows WLAN 工作线程异常结束".into()))?
    }
}

fn connect_wifi(ssid: &str) -> Result<(), CoreError> {
    let client = WlanClient::open()?;
    let interfaces = client.interfaces()?;
    if interfaces.is_empty() {
        return Err(CoreError::Wifi("没有找到可用的无线网卡".into()));
    }

    let mut last_error = None;
    for interface in interfaces {
        match client.connect(&interface.InterfaceGuid, ssid) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| CoreError::Wifi("没有可连接的无线网卡".into())))
}

struct WlanClient(HANDLE);

impl WlanClient {
    fn open() -> Result<Self, CoreError> {
        let mut negotiated = 0_u32;
        let mut handle = HANDLE::default();
        let result = unsafe { WlanOpenHandle(2, None, &mut negotiated, &mut handle) };
        if result != ERROR_SUCCESS.0 {
            return Err(wlan_error("无法打开 Windows Native Wi-Fi 服务", result));
        }
        Ok(Self(handle))
    }

    fn interfaces(&self) -> Result<Vec<WLAN_INTERFACE_INFO>, CoreError> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        let result = unsafe { WlanEnumInterfaces(self.0, None, &mut list) };
        if result != ERROR_SUCCESS.0 || list.is_null() {
            return Err(wlan_error("无法枚举无线网卡", result));
        }
        let count = unsafe { (*list).dwNumberOfItems as usize };
        let first =
            unsafe { std::ptr::addr_of!((*list).InterfaceInfo) as *const WLAN_INTERFACE_INFO };
        let interfaces = unsafe { std::slice::from_raw_parts(first, count) }.to_vec();
        unsafe { WlanFreeMemory(list.cast()) };
        Ok(interfaces)
    }

    fn connect(&self, interface: &GUID, ssid: &str) -> Result<(), CoreError> {
        if self.current_ssid(interface).as_deref() == Some(ssid) {
            return Ok(());
        }

        let profile_xml = open_profile_xml(ssid);
        let profile_wide = wide(&profile_xml);
        let mut reason = 0_u32;
        let result = unsafe {
            WlanSetProfile(
                self.0,
                interface,
                0,
                PCWSTR(profile_wide.as_ptr()),
                PCWSTR::null(),
                true,
                None,
                &mut reason,
            )
        };
        if result != ERROR_SUCCESS.0 {
            return Err(wlan_error_with_reason(
                "无法创建 HIT-WLAN 配置文件",
                result,
                reason,
            ));
        }

        let profile_name = wide(ssid);
        let parameters = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(profile_name.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let result = unsafe { WlanConnect(self.0, interface, &parameters, None) };
        if result != ERROR_SUCCESS.0 {
            return Err(wlan_error("Windows WLAN 拒绝连接请求", result));
        }

        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if self.current_ssid(interface).as_deref() == Some(ssid) {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(400));
        }
        Err(CoreError::Wifi(
            "连接请求已发出，但 15 秒内未连接成功".into(),
        ))
    }

    fn current_ssid(&self, interface: &GUID) -> Option<String> {
        let mut size = 0_u32;
        let mut data: *mut c_void = std::ptr::null_mut();
        let result = unsafe {
            WlanQueryInterface(
                self.0,
                interface,
                wlan_intf_opcode_current_connection,
                None,
                &mut size,
                &mut data,
                None,
            )
        };
        if result != ERROR_SUCCESS.0
            || data.is_null()
            || size < size_of::<WLAN_CONNECTION_ATTRIBUTES>() as u32
        {
            return None;
        }
        let attributes = unsafe { &*(data.cast::<WLAN_CONNECTION_ATTRIBUTES>()) };
        let output = if attributes.isState == wlan_interface_state_connected {
            let ssid = &attributes.wlanAssociationAttributes.dot11Ssid;
            let length = (ssid.uSSIDLength as usize).min(ssid.ucSSID.len());
            Some(String::from_utf8_lossy(&ssid.ucSSID[..length]).into_owned())
        } else {
            None
        };
        unsafe { WlanFreeMemory(data) };
        output
    }
}

impl Drop for WlanClient {
    fn drop(&mut self) {
        unsafe { WlanCloseHandle(self.0, None) };
    }
}

fn open_profile_xml(ssid: &str) -> String {
    let escaped = xml_escape(ssid);
    let hex = ssid
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    format!(
        r#"<?xml version="1.0"?><WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1"><name>{escaped}</name><SSIDConfig><SSID><hex>{hex}</hex><name>{escaped}</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>auto</connectionMode><MSM><security><authEncryption><authentication>open</authentication><encryption>none</encryption><useOneX>false</useOneX></authEncryption></security></MSM></WLANProfile>"#
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn wlan_error(stage: &str, code: u32) -> CoreError {
    CoreError::Wifi(format!("{stage}（Windows 错误码 {code}）"))
}

fn wlan_error_with_reason(stage: &str, code: u32, reason: u32) -> CoreError {
    CoreError::Wifi(format!(
        "{stage}（Windows 错误码 {code}，WLAN 原因码 {reason}）"
    ))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn set_launch_at_login(enabled: bool) -> io::Result<()> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::other(format!(
            "无法打开用户启动项（错误码 {}）",
            status.0
        )));
    }

    let result = if enabled {
        let executable = std::env::current_exe()?;
        let command = format!("\"{}\" --autostart", executable.display());
        let utf16 = wide(&command);
        let bytes = unsafe {
            std::slice::from_raw_parts(utf16.as_ptr().cast::<u8>(), utf16.len() * size_of::<u16>())
        };
        unsafe { RegSetValueExW(key, w!("HITAutoLogin"), None, REG_SZ, Some(bytes)) }
    } else {
        unsafe { RegDeleteValueW(key, w!("HITAutoLogin")) }
    };
    let _ = unsafe { RegCloseKey(key) };

    if result == ERROR_SUCCESS || (!enabled && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "无法更新用户启动项（错误码 {}）",
            result.0
        )))
    }
}

#[derive(Default)]
struct WindowsControls {
    window: nwg::Window,
    icon: nwg::Icon,
    tray: nwg::TrayNotification,
    tray_menu: nwg::Menu,
    menu_settings: nwg::MenuItem,
    menu_detect: nwg::MenuItem,
    menu_exit: nwg::MenuItem,
    title: nwg::Label,
    subtitle: nwg::Label,
    username_label: nwg::Label,
    username: nwg::TextInput,
    password_label: nwg::Label,
    password: nwg::TextInput,
    privacy: nwg::Label,
    launch_at_login: nwg::CheckBox,
    save: nwg::Button,
    save_detect: nwg::Button,
    status: nwg::Label,
    detail: nwg::Label,
    exit: nwg::Button,
    notice: nwg::Notice,
}

struct WindowsUi {
    controls: WindowsControls,
    controller: Arc<AppController>,
    mailbox: Arc<Mutex<VecDeque<StatusUpdate>>>,
    event_handler: RefCell<Option<nwg::EventHandler>>,
    raw_handler: RefCell<Option<nwg::RawEventHandler>>,
}

impl WindowsUi {
    fn build(
        controller: Arc<AppController>,
        receiver: mpsc::Receiver<AppEvent>,
    ) -> Result<Rc<Self>, nwg::NwgError> {
        let mut c = WindowsControls::default();
        nwg::Icon::builder()
            .source_system(Some(nwg::OemIcon::Information))
            .build(&mut c.icon)?;
        nwg::Window::builder()
            .flags(nwg::WindowFlags::WINDOW)
            .size((430, 405))
            .position((520, 220))
            .title("HIT 校园网自动登录")
            .icon(Some(&c.icon))
            .build(&mut c.window)?;
        nwg::TrayNotification::builder()
            .parent(&c.window)
            .icon(Some(&c.icon))
            .tip(Some("HIT 校园网自动登录"))
            .build(&mut c.tray)?;
        nwg::Menu::builder()
            .popup(true)
            .parent(&c.window)
            .build(&mut c.tray_menu)?;
        nwg::MenuItem::builder()
            .text("打开设置")
            .parent(&c.tray_menu)
            .build(&mut c.menu_settings)?;
        nwg::MenuItem::builder()
            .text("立即检测")
            .parent(&c.tray_menu)
            .build(&mut c.menu_detect)?;
        nwg::MenuItem::builder()
            .text("退出")
            .parent(&c.tray_menu)
            .build(&mut c.menu_exit)?;

        let config = controller.configuration();
        label(
            &mut c.title,
            &c.window,
            "HIT 校园网自动登录",
            (18, 16),
            (390, 28),
        )?;
        label(
            &mut c.subtitle,
            &c.window,
            "HIT-WLAN · wp.hit.edu.cn",
            (18, 43),
            (390, 22),
        )?;
        label(&mut c.username_label, &c.window, "学号", (18, 78), (70, 24))?;
        nwg::TextInput::builder()
            .parent(&c.window)
            .position((92, 74))
            .size((310, 28))
            .text(&config.username)
            .limit(64)
            .build(&mut c.username)?;
        label(
            &mut c.password_label,
            &c.window,
            "密码",
            (18, 118),
            (70, 24),
        )?;
        nwg::TextInput::builder()
            .parent(&c.window)
            .position((92, 114))
            .size((310, 28))
            .text(&config.password)
            .password(Some('●'))
            .limit(256)
            .build(&mut c.password)?;
        label(
            &mut c.privacy,
            &c.window,
            "账号密码以明文配置文件保存；日志不会记录认证敏感参数。",
            (18, 151),
            (390, 36),
        )?;
        nwg::CheckBox::builder()
            .parent(&c.window)
            .position((18, 188))
            .size((300, 28))
            .text("登录 Windows 时自动启动")
            .check_state(if config.launch_at_login {
                nwg::CheckBoxState::Checked
            } else {
                nwg::CheckBoxState::Unchecked
            })
            .build(&mut c.launch_at_login)?;
        nwg::Button::builder()
            .parent(&c.window)
            .position((18, 226))
            .size((105, 32))
            .text("保存")
            .build(&mut c.save)?;
        nwg::Button::builder()
            .parent(&c.window)
            .position((133, 226))
            .size((175, 32))
            .text("保存并立即检测")
            .build(&mut c.save_detect)?;
        nwg::Button::builder()
            .parent(&c.window)
            .position((323, 226))
            .size((79, 32))
            .text("退出")
            .build(&mut c.exit)?;
        label(
            &mut c.status,
            &c.window,
            "等待首次检测",
            (18, 282),
            (384, 28),
        )?;
        label(
            &mut c.detail,
            &c.window,
            "事件触发运行，无后台网络轮询",
            (18, 316),
            (384, 48),
        )?;
        nwg::Notice::builder()
            .parent(&c.window)
            .build(&mut c.notice)?;

        let mailbox = Arc::new(Mutex::new(VecDeque::new()));
        let ui = Rc::new(Self {
            controls: c,
            controller,
            mailbox: mailbox.clone(),
            event_handler: RefCell::new(None),
            raw_handler: RefCell::new(None),
        });

        let weak = Rc::downgrade(&ui);
        let handler =
            nwg::full_bind_event_handler(&ui.controls.window.handle, move |event, _, handle| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                use nwg::Event as E;
                match event {
                    E::OnWindowClose if handle == ui.controls.window => {
                        ui.controls.window.set_visible(false)
                    }
                    E::OnContextMenu if handle == ui.controls.tray => {
                        let (x, y) = nwg::GlobalCursor::position();
                        ui.controls.tray_menu.popup(x, y);
                    }
                    E::OnMousePress(nwg::MousePressEvent::MousePressLeftUp)
                        if handle == ui.controls.tray =>
                    {
                        ui.show_settings()
                    }
                    E::OnMenuItemSelected if handle == ui.controls.menu_settings => {
                        ui.show_settings()
                    }
                    E::OnMenuItemSelected if handle == ui.controls.menu_detect => {
                        ui.controller.trigger(RunTrigger::Manual)
                    }
                    E::OnMenuItemSelected if handle == ui.controls.menu_exit => {
                        nwg::stop_thread_dispatch()
                    }
                    E::OnButtonClick if handle == ui.controls.save => ui.save(false),
                    E::OnButtonClick if handle == ui.controls.save_detect => ui.save(true),
                    E::OnButtonClick if handle == ui.controls.exit => nwg::stop_thread_dispatch(),
                    E::OnNotice if handle == ui.controls.notice => ui.drain_status(),
                    _ => {}
                }
            });
        *ui.event_handler.borrow_mut() = Some(handler);

        let power_controller = ui.controller.clone();
        let raw = nwg::bind_raw_event_handler(
            &ui.controls.window.handle,
            0x10_001,
            move |_, msg, wparam, _| {
                if msg == WM_POWERBROADCAST && wparam == PBT_APMRESUMEAUTOMATIC {
                    power_controller.trigger(RunTrigger::Resume);
                }
                None
            },
        )?;
        *ui.raw_handler.borrow_mut() = Some(raw);

        let notice = ui.controls.notice.sender();
        thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                let AppEvent::Status(status) = event;
                if let Ok(mut queue) = mailbox.lock() {
                    queue.push_back(status);
                }
                notice.notice();
            }
        });

        if !config.credentials_present() {
            ui.show_settings();
        }
        Ok(ui)
    }

    fn show_settings(&self) {
        self.controls.window.set_visible(true);
        self.controls.window.set_focus();
    }

    fn save(&self, detect: bool) {
        let mut configuration = self.controller.configuration();
        configuration.username = self.controls.username.text();
        configuration.password = self.controls.password.text();
        configuration.launch_at_login =
            self.controls.launch_at_login.check_state() == nwg::CheckBoxState::Checked;
        match self.controller.save(configuration) {
            Ok(()) if detect => self.controller.trigger(RunTrigger::Manual),
            Ok(()) => {}
            Err(error) => {
                nwg::modal_error_message(&self.controls.window, "无法保存设置", &error.to_string());
            }
        }
    }

    fn drain_status(&self) {
        let Ok(mut queue) = self.mailbox.lock() else {
            return;
        };
        while let Some(status) = queue.pop_front() {
            self.controls.status.set_text(&status.message);
            self.controls.detail.set_text(&status.detail);
            self.controls.save_detect.set_enabled(!status.running);
            self.controls.menu_detect.set_enabled(!status.running);
            self.controls
                .tray
                .set_tip(&format!("HIT 校园网：{}", status.message));
            if matches!(status.level, StatusLevel::Failure) {
                self.controls.tray.show(
                    &status.message,
                    Some("HIT 校园网检测失败"),
                    Some(
                        nwg::TrayNotificationFlags::ERROR_ICON | nwg::TrayNotificationFlags::QUIET,
                    ),
                    None,
                );
            }
        }
    }
}

impl Drop for WindowsUi {
    fn drop(&mut self) {
        if let Some(handler) = self.event_handler.borrow_mut().take() {
            nwg::unbind_event_handler(&handler);
        }
        if let Some(handler) = self.raw_handler.borrow_mut().take() {
            let _ = nwg::unbind_raw_event_handler(&handler);
        }
    }
}

fn label(
    out: &mut nwg::Label,
    parent: &nwg::Window,
    text: &str,
    position: (i32, i32),
    size: (i32, i32),
) -> Result<(), nwg::NwgError> {
    nwg::Label::builder()
        .parent(parent)
        .position(position)
        .size(size)
        .text(text)
        .build(out)
}

pub fn run() -> Result<(), AppError> {
    let _instance = SingleInstance::acquire()
        .map_err(|error| AppError::Platform(error.to_string()))?
        .ok_or(AppError::AlreadyRunning)?;
    nwg::init().map_err(|error| AppError::Platform(error.to_string()))?;
    let _ = nwg::Font::set_global_family("Microsoft YaHei UI");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| AppError::Platform(error.to_string()))?;
    let (sender, receiver) = mpsc::channel();
    let controller = Arc::new(AppController::new(runtime.handle().clone(), sender)?);
    let _ui = WindowsUi::build(controller.clone(), receiver)
        .map_err(|error| AppError::Platform(error.to_string()))?;
    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        controller.trigger(RunTrigger::Launch);
    });
    nwg::dispatch_thread_events();
    Ok(())
}

pub fn show_fatal_error(message: &str) {
    if nwg::init().is_ok() {
        nwg::simple_message("HIT 校园网自动登录", message);
    }
}
