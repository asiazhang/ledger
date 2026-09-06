#!/bin/sh
# 一键质量检查：前端类型检查 + 守门脚本与测试类型检查 + Rust clippy + Rust fmt 检查 + 文档一致性检查 + 命令注册一致性检查 + 结构守门检查 + i18n key 全等检查 + 参考数据测试桩守门检查
# 任一环节失败即退出（CI 可直接调用）
set -eu
cd "$(dirname "$0")/.."

# 守门脚本运行时 = Bun（issue #734 / ADR-0083）：缺失即显式报错，不静默降级
if ! command -v bun >/dev/null 2>&1; then
  echo "✗ 未检测到 bun：守门脚本以 Bun 运行时执行（ADR-0083），请安装 bun（CI 固定 1.4.0）后重试" >&2
  exit 1
fi

echo "▶ 前端类型检查 (pnpm exec vue-tsc --noEmit)"
pnpm exec vue-tsc --noEmit

echo "▶ 守门脚本与测试类型检查 (pnpm exec vue-tsc -p tsconfig.scripts.json --noEmit)"
pnpm exec vue-tsc -p tsconfig.scripts.json --noEmit

echo "▶ Rust clippy (--all-targets --all-features, -D warnings)"
( cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings )

echo "▶ Rust 格式检查 (cargo fmt --check)"
( cd src-tauri && cargo fmt -- --check )

./scripts/check-docs.sh

echo "▶ 命令注册一致性检查 (bun scripts/check-commands.ts)"
bun scripts/check-commands.ts

echo "▶ 结构守门检查 (bun scripts/check-structure.ts)"
bun scripts/check-structure.ts

echo "▶ i18n key 全等检查 (bun scripts/check-i18n-keys.ts)"
bun scripts/check-i18n-keys.ts

echo "▶ 参考数据测试桩守门检查 (bun scripts/check-test-stubs.ts)"
bun scripts/check-test-stubs.ts

echo "✅ 所有检查通过"
