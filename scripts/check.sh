#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

if ! command -v bun >/dev/null 2>&1; then
  echo "✗ 未检测到 bun：守门脚本以 Bun 运行时执行（ADR-0083），请安装 bun（CI 固定 1.4.4）后重试" >&2
  exit 1
fi

echo "▶ 前端类型检查 (pnpm exec vue-tsc --noEmit)"
pnpm exec vue-tsc --noEmit

echo "▶ 守门脚本与测试类型检查 (pnpm exec vue-tsc -p tsconfig.scripts.json --noEmit)"
pnpm exec vue-tsc -p tsconfig.scripts.json --noEmit

echo "▶ 前端 lint (pnpm exec oxlint --deny-warnings)"
pnpm exec oxlint --deny-warnings

echo "▶ 前端格式检查 (pnpm exec oxfmt --check)"
pnpm exec oxfmt --check .

echo "▶ Rust clippy (--workspace --all-targets --all-features, -D warnings)"
( cd src-tauri && cargo clippy --workspace --all-targets --all-features -- -D warnings )

echo "▶ Rust gate-off 编译检查 (cargo check -p ledger-infra，默认 feature 不含 axum)"
( cd src-tauri && cargo check -p ledger-infra )

echo "▶ Rust 格式检查 (cargo fmt --all --check)"
( cd src-tauri && cargo fmt --all -- --check )

echo "▶ 基础设施 rustdoc 门禁 (cargo doc -p ledger-infra --no-deps, -D warnings)"
( cd src-tauri && RUSTDOCFLAGS='-D warnings' cargo doc -p ledger-infra --no-deps )

./scripts/check-docs.sh

echo "▶ 命令注册一致性检查 (bun scripts/check-commands.ts)"
bun scripts/check-commands.ts

echo "▶ 版本元数据一致性检查 (bun scripts/check-versions.ts)"
bun scripts/check-versions.ts

echo "▶ 结构守门检查 (bun scripts/check-structure.ts)"
bun scripts/check-structure.ts

echo "▶ 前端 workspace 结构守门 (bun scripts/check-frontend-structure.ts)"
bun scripts/check-frontend-structure.ts

echo "▶ 基础设施账本数据表 DML 禁令 (bun scripts/check-infra-dml.ts)"
bun scripts/check-infra-dml.ts

echo "▶ 东财行情面端点零残留源码扫描 (bun scripts/check-eastmoney-residue.ts)"
bun scripts/check-eastmoney-residue.ts

echo "▶ 后台服务成对拉起守门 (bun scripts/check-background-services.ts)"
bun scripts/check-background-services.ts

echo "▶ 样式块守门检查 (bun scripts/check-style-blocks.ts)"
bun scripts/check-style-blocks.ts

echo "▶ i18n key 全等检查 (bun scripts/check-i18n-keys.ts)"
bun scripts/check-i18n-keys.ts

echo "▶ 弹窗表单节奏守门 (bun scripts/check-dialog-forms.ts)"
bun scripts/check-dialog-forms.ts

echo "▶ 测试桩守门检查 (bun scripts/check-test-stubs.ts)"
bun scripts/check-test-stubs.ts

echo "▶ Rust 测试守门检查 (bun scripts/check-test-support.ts)"
bun scripts/check-test-support.ts

echo "▶ 测试执行覆盖守门 (bun scripts/test-exec.ts check)"
bun scripts/test-exec.ts check

echo "▶ 前端异步守门检查 (bun scripts/check-async-guards.ts)"
bun scripts/check-async-guards.ts

echo "▶ 注释禁令棘轮守门 (bun scripts/check-comment-gate.ts)"
bun scripts/check-comment-gate.ts

echo "▶ 守门脚本测试 (bun test scripts/)"
bun test scripts/

echo "✅ 所有检查通过"
