#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

echo "▶ 清理前端产物 (dist, .vite)"
rm -rf dist dist-ssr .vite

echo "▶ 清理 Rust target (cargo clean)"
( cd src-tauri && cargo clean )

echo "✅ 清理完成"
