#!/bin/sh
# 构建发布版桌面应用（先构建前端，再编译 Rust 并打包）
set -eu
cd "$(dirname "$0")/.."
exec pnpm run tauri build
