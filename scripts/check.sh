#!/bin/sh
# 一键质量检查：前端类型检查 + Rust clippy + Rust fmt 检查 + 文档一致性检查
# 任一环节失败即退出（CI 可直接调用）
set -eu
cd "$(dirname "$0")/.."

echo "▶ 前端类型检查 (vue-tsc --noEmit)"
npx vue-tsc --noEmit

echo "▶ Rust clippy (--all-targets --all-features, -D warnings)"
( cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings )

echo "▶ Rust 格式检查 (cargo fmt --check)"
( cd src-tauri && cargo fmt -- --check )

./scripts/check-docs.sh

echo "✅ 所有检查通过"
