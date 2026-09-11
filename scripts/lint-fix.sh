#!/bin/sh
# 自动修复 Rust 代码格式与 clippy 警告
set -eu
cd "$(dirname "$0")/.."
# --workspace / --all：非虚拟 workspace 下默认只作用于根包，缺范围参数会漏修成员 crate。
( cd src-tauri && cargo fmt --all && cargo clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged )
echo "✅ 已自动格式化并尝试修复 clippy 警告"
