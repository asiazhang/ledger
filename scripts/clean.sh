#!/bin/sh
# 清理构建产物：前端 dist / Vite 缓存 + Rust target
set -eu
cd "$(dirname "$0")/.."

echo "▶ 清理前端产物 (dist, .vite)"
rm -rf dist dist-ssr .vite

echo "▶ 清理 Rust target (cargo clean)"
( cd src-tauri && cargo clean )

echo "✅ 清理完成"
