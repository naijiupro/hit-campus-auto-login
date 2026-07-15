#!/bin/zsh

set -euo pipefail

ROOT_DIR="${0:A:h:h}"
CONFIGURATION="${1:-release}"
APP_NAME="HIT 校园网自动登录.app"
APP_PATH="$ROOT_DIR/dist/$APP_NAME"

cd "$ROOT_DIR"
swift build -c "$CONFIGURATION"
BIN_PATH="$(swift build -c "$CONFIGURATION" --show-bin-path)"

rm -rf "$APP_PATH"
mkdir -p "$APP_PATH/Contents/MacOS"
cp "$BIN_PATH/HITAutoLogin" "$APP_PATH/Contents/MacOS/HITAutoLogin"
cp "$ROOT_DIR/App/Info.plist" "$APP_PATH/Contents/Info.plist"

/usr/bin/codesign --force --deep --sign - "$APP_PATH"

echo "$APP_PATH"
