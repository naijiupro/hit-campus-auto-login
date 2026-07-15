#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

say() { printf '\n==> %s\n' "$*"; }
ask() {
    local prompt="$1" default="${2:-y}" answer
    if [[ "$default" == y ]]; then
        read -r -p "$prompt [Y/n] " answer || true
        [[ -z "$answer" || "$answer" =~ ^[Yy]([Ee][Ss])?$ ]]
    else
        read -r -p "$prompt [y/N] " answer || true
        [[ "$answer" =~ ^[Yy]([Ee][Ss])?$ ]]
    fi
}

install_dependencies() {
    if command -v apt-get >/dev/null 2>&1; then
        sudo apt-get update
        sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
            build-essential pkg-config libgtk-3-dev network-manager curl ca-certificates
    elif command -v dnf >/dev/null 2>&1; then
        sudo dnf install -y gcc gcc-c++ make pkgconf-pkg-config gtk3-devel \
            NetworkManager curl ca-certificates
    else
        echo '无法识别包管理器。请手动安装 GTK 3 开发包、pkg-config、编译工具和 NetworkManager。' >&2
        exit 1
    fi
}

say '检查 Linux 构建依赖'
if ! command -v pkg-config >/dev/null 2>&1 || ! pkg-config --exists gtk+-3.0 || ! command -v nmcli >/dev/null 2>&1; then
    ask '缺少 GTK/NetworkManager 构建依赖，是否现在安装？' y && install_dependencies
fi

if ! command -v cargo >/dev/null 2>&1; then
    ask '未找到 Rust，是否使用 rustup 安装到当前用户？' y || exit 1
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
fi

say '编译并运行测试'
cd "$ROOT"
cargo test --workspace
cargo build --release -p hit-auto-login

say '安装到当前用户目录'
mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"
install -m 0755 target/release/hit-auto-login "$HOME/.local/bin/hit-auto-login"
cat > "$HOME/.local/share/applications/hit-auto-login.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=HIT 校园网自动登录
Comment=事件驱动的 HIT-WLAN Srun 自动认证
Exec=$HOME/.local/bin/hit-auto-login
Icon=network-wireless
Terminal=false
Categories=Network;Utility;
EOF

say '交互配置账号和登录启动'
"$HOME/.local/bin/hit-auto-login" --configure

if [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]] && ask '现在启动托盘程序？' y; then
    systemctl --user start hit-auto-login.service 2>/dev/null || \
        nohup "$HOME/.local/bin/hit-auto-login" >/dev/null 2>&1 &
fi

say '安装完成'
echo '命令：hit-auto-login --configure / hit-auto-login --check-once'
echo '应用菜单：HIT 校园网自动登录'

