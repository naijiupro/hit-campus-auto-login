# HIT 校园网自动登录（Windows / Linux）

这是对齐 macOS 1.0.2 门户兼容性修复的共享 Rust 实现，目标平台为 Windows 10/11 与使用 NetworkManager、systemd 的主流 Linux 桌面。程序是事件驱动的：登录启动、系统恢复、屏幕恢复或用户手动操作时执行一次有限流程，空闲时不循环探测网络。

选择 Rust 的原因是认证算法可以只维护一份，同时能生成不依赖 Electron 或 Python 运行时的原生可执行文件。平台层只负责 Wi‑Fi、启动项、系统事件和界面。

## Linux 一键交互安装

Ubuntu 22.04+、Debian 系和 Fedora 可在源码目录执行：

```bash
bash cross-platform/scripts/install-linux.sh
```

脚本会交互完成：

1. 检查并按需安装 GTK 3、NetworkManager 和 Rust 构建依赖；
2. 运行全部测试并编译 release 可执行文件；
3. 安装到 `~/.local/bin/hit-auto-login`；
4. 启动纯 CLI 配置，询问学号、密码和登录启动选项；
5. 可选地立即执行一次联网检测并启动托盘程序。

日常 CLI 命令：

```bash
hit-auto-login --configure
hit-auto-login --check-once
```

卸载：

```bash
bash cross-platform/scripts/uninstall-linux.sh
```

配置保存在 `~/.config/hit-auto-login/config.json`，权限为 `0600`。自动启动使用 `~/.config/systemd/user/hit-auto-login.service`，`Restart=no`，退出后不会被立即拉起。

Linux 图形模式使用 GTK 3 设置窗口和 StatusNotifierItem 托盘；左键托盘图标打开设置，菜单提供“立即检测”和“退出”。

## Windows 构建与安装

安装 Rust stable 和 Visual Studio 2022 C++ Build Tools 后，在 PowerShell 执行：

```powershell
cross-platform\scripts\build-windows.ps1
```

产物位于 `cross-platform\dist\windows`：

- `hit-auto-login.exe`：可直接运行；
- `HITAutoLogin-Windows-x64.zip`：便携发布包；
- 如果 PATH 中存在 Inno Setup 的 `ISCC.exe`，还会生成 `HITAutoLogin-Setup-x64.exe` 用户级安装包。

也可以直接使用用户级安装脚本：

```powershell
cross-platform\scripts\install-windows.ps1
```

程序安装到 `%LOCALAPPDATA%\Programs\HITAutoLogin`。配置保存在 `%APPDATA%\HITAutoLogin\config.json`。登录启动使用 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`，设置界面可以随时启用或关闭。

v1.0.2 修复了程序启动时提示“无法定位程序输入点 `GetWindowSubclass`”的问题。Windows 构建现在会嵌入 `Microsoft.Windows.Common-Controls` 6.0 manifest，并在打包前从 EXE 资源中读取和验证该声明；缺失时构建脚本和 CI 会直接失败，不再生成不可启动的发布包。设置窗口同时改为简洁的高 DPI 布局和 Wi-Fi 图标，网络探测使用隐藏子进程，不再闪现命令行窗口。

Windows 使用 `wlanapi.dll` Native Wi‑Fi API，不解析 `netsh` 的本地化文本；如果不存在 HIT-WLAN 配置，会创建开放网络 WLAN Profile。恢复事件由窗口消息 `WM_POWERBROADCAST` 的 `PBT_APMRESUMEAUTOMATIC` 触发。

## 实际网络流程

每个触发事件只运行一次，且同一时间最多一个任务：

1. `ping baidu.com`，失败时补充请求 `https://www.baidu.com/robots.txt`；
2. HTTPS 最终 URL 必须仍是 `baidu.com` 或其子域，否则视为门户重定向；
3. 已联网则立即返回，不读取 SSID、不切换 Wi‑Fi、不访问校园门户；
4. 未联网时请求连接开放网络 `HIT-WLAN`；
5. 再次验证公网；
6. 仍未联网时，从 `https://wp.hit.edu.cn/` 动态读取 `ac_id` 和 `user_ip`，执行 Srun challenge/login；
7. 最多验证公网三次，成功或明确失败后结束。

HTTP、ping、Wi‑Fi、门户认证和系统命令均有超时。同一进程内的重复恢复事件有 20 秒冷却；任务运行中到达的其他触发直接忽略。程序没有常驻定时网络循环。

## 共享认证核心

`crates/hit-auto-login-core` 是 Windows/Linux 共用实现：

- HMAC-MD5 与 `{MD5}` 密码；
- JavaScript UTF‑16 `charCodeAt` 语义的 XEncode；
- Srun 自定义 Base64；
- 固定字段顺序的 info JSON；
- 按 macOS 参考顺序拼接的 SHA1 checksum；
- HTML 字段、JSON/JSONP 解析；
- 门户客户端、公网检测、工作流和并发/冷却协调器。

平台代码位于 `crates/hit-auto-login-app/src/platform/`，不会复制认证算法。

## 测试

```bash
cd cross-platform
cargo test --workspace
```

测试使用合成账号 `2024000000`，覆盖：

- HMAC-MD5、SHA1、XEncode、自定义 Base64；
- 完整 Srun 参数与 JavaScript 参考值；
- 非 ASCII 密码；
- `ac_id`/`user_ip` 与 JSON/JSONP 解析；
- 已联网时跳过 Wi‑Fi 和认证门户；
- 多事件并发锁与恢复事件去重；
- 公网、Wi‑Fi 和门户认证超时；
- 门户错误的用户提示与 URL 清理。

GitHub Actions 配置会分别在 Ubuntu 22.04 和 Windows Server 2022 构建 release 产物。

## 隐私和日志

账号密码按需求明文保存，但不会写入项目、测试或日志。程序不输出密码、challenge、完整 info、完整认证 URL；认证请求的底层网络错误会被替换为阶段化信息，门户消息中的 URL 会被清理并截断。当前版本默认不创建持久日志文件。

## 门户认证故障排查

### 门户返回 `login_error` 或界面曾只显示“认证失败：0”

v1.0.1 修复了以下协议兼容问题：

- 所有 GET 查询参数均按 `application/x-www-form-urlencoded` 规则逐项编码，Srun `info` 中的原始 `+`、`/`、`=` 分别发送为 `%2B`、`%2F`、`%3D`；
- 获取 challenge 后优先使用响应中的 `client_ip`，仅在缺失时回退到门户 HTML 的 `user_ip`；
- 门户首页、challenge 和登录请求共用一个保留 Cookie 的 HTTP Client，并发送桌面浏览器风格的 `Accept`、`Accept-Language`、`Referer` 和平台 User-Agent；
- 数值 `0`、字符串 `"0"`、空字符串和笼统的 `fail` 不再作为最终错误消息；`E2553` 等常见错误码会显示中文解释。

如果仍然失败，请只记录阶段、`error`/非零 `ecode` 和清理后的提示，不要复制完整认证 URL、密码、challenge、info 或 chksum。

## 真实校园网验收

仓库外的真实 HIT-WLAN 环境仍需作为发布前最终步骤。请复制 [E2E_TEST_RECORD.md](E2E_TEST_RECORD.md) 记录操作系统、Wi‑Fi 后端、登录前状态、门户结果和公网验证；不要记录账号、密码、challenge、完整 URL 或客户端 IP。

已知限制见 [KNOWN_LIMITATIONS.md](KNOWN_LIMITATIONS.md)。
