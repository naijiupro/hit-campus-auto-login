#!/usr/bin/env bash
set -euo pipefail

systemctl --user disable --now hit-auto-login.service 2>/dev/null || true
rm -f "$HOME/.config/systemd/user/hit-auto-login.service"
systemctl --user daemon-reload 2>/dev/null || true
rm -f "$HOME/.local/bin/hit-auto-login"
rm -f "$HOME/.local/share/applications/hit-auto-login.desktop"
read -r -p '是否同时删除明文账号配置？[y/N] ' answer || true
if [[ "$answer" =~ ^[Yy]([Ee][Ss])?$ ]]; then
    rm -rf "$HOME/.config/hit-auto-login"
fi
echo '卸载完成。'

