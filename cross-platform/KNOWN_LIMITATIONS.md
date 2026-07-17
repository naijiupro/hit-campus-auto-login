# 已知限制

- v1.0.1 已修复 Srun 查询参数中原始 `+` 被 CGI 当作空格、challenge IP 优先级以及 `ecode=0` 错误显示问题；如果校园门户再次变更字段或协议，需要重新抓取不含个人信息的最小响应样本。

- 当前环境无法代替真实 HIT-WLAN 完成最终端到端认证；门户算法已用 macOS/网页 JavaScript 参考向量验证，但发布前仍需现场记录。
- Linux 第一版按需求允许的可靠路径使用 `nmcli`，关键操作只依赖退出状态且强制 `LC_ALL=C`，不解析本地化说明文本。后续可替换为 NetworkManager D‑Bus 原生调用而不改共享核心。
- GNOME Shell 默认可能不显示 StatusNotifierItem，需要发行版自带或用户启用 AppIndicator/KStatusNotifierItem 扩展。CLI 配置和 `--check-once` 不受影响。
- Linux 屏幕恢复监听使用 `org.freedesktop.ScreenSaver.ActiveChanged(false)`；不提供该接口的桌面仍能通过 systemd-logind 的睡眠恢复事件和手动检测工作。
- systemd 用户服务依赖图形会话向 user manager 提供 `DISPLAY`/`WAYLAND_DISPLAY` 和会话 D‑Bus 环境。极简窗口管理器可能需要在登录脚本中执行 `systemctl --user import-environment DISPLAY WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS`。
- Windows 可执行文件和 Inno Setup 包默认未进行 Authenticode 签名，首次运行可能显示 SmartScreen 提示。正式分发应使用组织代码签名证书。
- Windows Native Wi‑Fi 连接受系统 WLAN 服务、组织策略、驱动和无线电硬件开关约束；界面只显示清理后的阶段与数字错误码。
- 当前构建目标为 x86_64。ARM64 可从源码构建，但发布脚本尚未生成 ARM64 安装包。
