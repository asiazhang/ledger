#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

if ! command -v bun >/dev/null 2>&1; then
  echo "✗ 未检测到 bun：测试执行器以 Bun 运行时执行（ADR-0083），请安装 bun（CI 固定 1.4.4）后重试" >&2
  exit 1
fi

bun scripts/test-exec.ts check

bun scripts/test-exec.ts

( cd src-tauri && cargo test --workspace --test e2e )

( cd src-tauri && cargo test --workspace --doc )
