#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

command -v cargo >/dev/null 2>&1 || { echo '未找到 Cargo，请先安装 Rust：https://rustup.rs/' >&2; exit 1; }
cargo fmt --all -- --check
cargo test --workspace
cargo build --release -p hit-auto-login

mkdir -p dist/linux
install -m 0755 target/release/hit-auto-login dist/linux/hit-auto-login
tar -C dist/linux -czf dist/linux/HITAutoLogin-Linux-x86_64.tar.gz hit-auto-login
sha256sum dist/linux/hit-auto-login dist/linux/HITAutoLogin-Linux-x86_64.tar.gz > dist/linux/SHA256SUMS
echo "$ROOT/dist/linux"

