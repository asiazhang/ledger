#!/bin/sh
# 启动完整开发环境：Vite 前端 + Rust 后端，均支持热重载
# 日常开发主命令
set -eu
cd "$(dirname "$0")/.."
exec npm run tauri dev
