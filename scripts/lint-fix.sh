#!/bin/sh
# 自动修复 Rust 代码格式与 clippy 警告
set -eu
cd "$(dirname "$0")/.."
( cd src-tauri && cargo fmt && cargo clippy --fix --all-targets --all-features --allow-dirty --allow-staged )
echo "✅ 已自动格式化并尝试修复 clippy 警告"
