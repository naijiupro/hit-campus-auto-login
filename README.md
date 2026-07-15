# HIT 校园网自动登录

事件驱动的 HIT-WLAN Srun 自动认证客户端，包含 macOS、Windows 10/11 和 Linux（Ubuntu/Debian/Fedora）版本。

- Windows/Linux、Linux CLI 一键安装和共享 Rust 认证核心见 [`cross-platform/README.md`](cross-platform/README.md)。
- 下文是原有 macOS 菜单栏版本说明。

> 登录前 HTML 和登录后 WebArchive 只作为本地协议分析样本，不纳入 Git 仓库，因为归档可能包含真实学号和校园网 IP。

## macOS 版本

一个事件驱动的轻量菜单栏应用。它不会持续轮询网络，只会在以下事件发生时执行一次完整流程：

- 应用随 Mac 登录启动；
- Mac 从睡眠中唤醒；
- 屏幕从熄屏中唤醒或用户会话恢复；
- 用户在菜单中点击“保存并立即检测”。

每次触发的流程是：先 `ping baidu.com`（HTTPS 作为 ICMP 被禁时的补充）→ 已联网则立即结束 → 未联网时确认当前 Wi‑Fi 并在必要时连接 `HIT-WLAN` → 调用 `https://wp.hit.edu.cn/` 的 Srun 认证接口 → 再次验证互联网。

实际执行时会先验证互联网是否已经可用。这样可避开 macOS 15 及更高版本偶尔隐藏当前 Wi‑Fi 名称的问题；如果已经联网，应用会直接结束，不会重复切换 Wi‑Fi。

## 构建

要求 macOS 13 或更高版本，以及 Xcode Command Line Tools。

```bash
./scripts/build-app.sh
```

生成的应用位于：

```text
dist/HIT 校园网自动登录.app
```

建议先把应用移动到 `~/Applications` 或 `/Applications`，再首次打开。应用没有 Dock 图标，入口在菜单栏的 Wi‑Fi 图标中。

## 首次使用

1. 打开应用并点击菜单栏图标。
2. 输入学号和密码。
3. 保持“登录 Mac 时自动启动”开启。
4. 点击“保存并立即检测”。

开机启动项保存在：

```text
~/Library/LaunchAgents/cn.edu.hit.HITAutoLogin.plist
```

学号和密码按需求使用普通 `UserDefaults` 保存，不使用钥匙串、不加密。对应偏好设置域为 `cn.edu.hit.HITAutoLogin`。如果设备可能被他人使用，请不要采用这种存储方式。

## 实现依据

项目中的登录前 HTML 和登录后 WebArchive 显示，HIT 门户使用 Srun 认证协议。应用复现了页面脚本中的实际流程：

1. 从实时门户页面读取 `ac_id` 和客户端 IP；
2. 请求 `/cgi-bin/get_challenge` 获取动态 challenge；
3. 生成 HMAC-MD5 密码、XEncode 信息、自定义 Base64 和 SHA1 校验值；
4. 请求 `/cgi-bin/srun_portal`；
5. 验证公网连通性。

因此应用不需要嵌入浏览器，也不会依赖网页按钮或固定客户端 IP。

## 测试

```bash
swift test
```

单元测试覆盖 HMAC-MD5、SHA1、门户字段解析和 JSONP 响应解析。完整认证仍需在连接 HIT-WLAN 的环境中进行。
