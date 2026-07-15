#!/usr/bin/env bash
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$HERE/hit-auto-login"

[[ -x "$BIN" ]] || { echo '安装包不完整：缺少 hit-auto-login' >&2; exit 1; }

if ! command -v nmcli >/dev/null 2>&1 || ! ldconfig -p 2>/dev/null | grep -q 'libgtk-3.so'; then
    read -r -p '缺少 GTK 3 或 NetworkManager 运行依赖，是否现在安装？[Y/n] ' answer || true
    if [[ -z "$answer" || "$answer" =~ ^[Yy]([Ee][Ss])?$ ]]; then
        if command -v apt-get >/dev/null 2>&1; then
            sudo apt-get update
            sudo DEBIAN_FRONTEND=noninteractive apt-get install -y libgtk-3-0 network-manager
        elif command -v dnf >/dev/null 2>&1; then
            sudo dnf install -y gtk3 NetworkManager
        else
            echo '无法识别包管理器，请手动安装 GTK 3 和 NetworkManager。' >&2
            exit 1
        fi
    fi
fi

mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"
install -m 0755 "$BIN" "$HOME/.local/bin/hit-auto-login"
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

echo '程序已安装，开始交互配置。'
"$HOME/.local/bin/hit-auto-login" --configure
echo '安装完成。之后可运行 hit-auto-login --check-once，或从应用菜单打开托盘程序。'

