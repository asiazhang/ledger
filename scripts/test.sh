#!/bin/sh
# 运行 Rust 单元测试
set -eu
cd "$(dirname "$0")/.."
( cd src-tauri && cargo test --all )
