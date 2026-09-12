#!/bin/sh
# 一键质量检查：前端类型检查 + 守门脚本与测试类型检查 + 前端 oxlint lint（--deny-warnings）+ Rust clippy + Rust gate-off 编译检查（infra 默认 feature 不含 axum，issue #1133）+ Rust fmt 检查 + 文档一致性检查 + 命令注册一致性检查 + 结构守门检查 + 后台服务成对拉起守门（issue #961）+ i18n key 全等检查 + 弹窗表单行距节奏守门（issue #804 / ADR-0079 决策 4）+ 测试桩守门检查（参考数据手搓桩 + 同回调重复桩）+ Rust 测试守门检查（禁直连建库 + 禁夹具裸 SQL + 禁默认时刻字面量，纯禁令，issue #752 落地 / #758 收口 / ADR-0084）+ 前端异步守门检查（手搓竞态序号硬零容忍 + catch 直弹 toast 基线冻结，issue #1039 / #1008 决议 4）
# Rust 侧命令必须显式声明 workspace 范围（spec #1086 / issue #1087）：非虚拟
# workspace 下 cargo 默认只作用于根包，缺范围参数会静默漏检成员 crate。
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

echo "▶ 前端 lint (pnpm exec oxlint --deny-warnings)"
pnpm exec oxlint --deny-warnings

echo "▶ Rust clippy (--workspace --all-targets --all-features, -D warnings)"
( cd src-tauri && cargo clippy --workspace --all-targets --all-features -- -D warnings )

# infra 默认 feature 编译门（issue #1133）：clippy 走 --all-features（http 门恒开、
# axum 恒在），门内 cfg 恒被编译——gate-off 形态（默认 feature 不含 axum）只有这
# 里核到：error.rs 在无 axum 依赖图下必须独立成立，误在门外引用 axum 即红。
echo "▶ Rust gate-off 编译检查 (cargo check -p ledger-infra，默认 feature 不含 axum)"
( cd src-tauri && cargo check -p ledger-infra )

echo "▶ Rust 格式检查 (cargo fmt --all --check)"
( cd src-tauri && cargo fmt --all -- --check )

./scripts/check-docs.sh

echo "▶ 命令注册一致性检查 (bun scripts/check-commands.ts)"
bun scripts/check-commands.ts

echo "▶ 结构守门检查 (bun scripts/check-structure.ts)"
bun scripts/check-structure.ts

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

echo "▶ 前端异步守门检查 (bun scripts/check-async-guards.ts)"
bun scripts/check-async-guards.ts

echo "✅ 所有检查通过"
