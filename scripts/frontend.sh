#!/bin/sh
# 仅启动前端 Vite dev server（端口 1420，无 Rust 后端）
# 适用于纯 UI 调试——此时 invoke 调用会失败
set -eu
cd "$(dirname "$0")/.."
exec pnpm run dev
