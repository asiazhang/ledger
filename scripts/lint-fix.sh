#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
( cd src-tauri && cargo fmt --all && cargo clippy --fix --workspace --all-targets --all-features --allow-dirty --allow-staged )
echo "✅ 已自动格式化并尝试修复 clippy 警告"
