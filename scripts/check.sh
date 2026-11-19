#!/bin/sh
# 一键质量检查：执行序列保持手写；质量门清单、挂载与 issue/ADR 出处的唯一事实源
# 是守门挂载登记 scripts/gate_mounts.py（issue #1682）——新增/删除门须同步登记，
# 与本文件及 CI workflow 的双向全等核对住 scripts/test_gate_mounts.py（删宿主挂载
# 行或删登记条目即红；核对测试文件本身的删除靠评审——import 为单向互证）。
# Rust 侧命令必须显式声明 workspace 范围（spec #1086 / issue #1087）：非虚拟
# workspace 下 cargo 默认只作用于根包，缺范围参数会静默漏检成员 crate。
# 任一环节失败即退出（CI 可直接调用）
#
# Web 参照前端已随 #2171/#2172 整体退役（ADR-0148）：vue-tsc×2、oxlint、oxfmt、
# check-frontend-structure、check-style-blocks、check-dialog-forms、
# check-test-stubs、check-async-guards 八门随宿主消失。
# 守门脚本运行时已迁移 uv + Python（#2174 / ADR-0083 修订注记）：守门脚本门宿主
# 行 = uv run scripts/<name>.py；类型检查门宿主 = scripts/check_scripts_types.py
# （mypy，PEP 723 内联依赖钉死版本）；守门脚本测试 = scripts/run_tests.py（pytest）。
set -eu
cd "$(dirname "$0")/.."

# 守门脚本运行时 = uv（issue #2174 / ADR-0083 修订注记）：缺失即显式报错，不静默降级
if ! command -v uv >/dev/null 2>&1; then
  echo "✗ 未检测到 uv：守门脚本以 uv 运行时执行（ADR-0083 修订注记 / issue #2174），请安装 uv（CI 固定 0.13.8）后重试" >&2
  exit 1
fi

echo "▶ 守门脚本与测试类型检查 (uv run scripts/check_scripts_types.py)"
uv run scripts/check_scripts_types.py

echo "▶ Rust clippy (--workspace --all-targets --all-features, -D warnings)"
( cd src-app && cargo clippy --workspace --all-targets --all-features -- -D warnings )

# infra 默认 feature 编译门（issue #1133）：clippy 走 --all-features（http 门恒开、
# axum 恒在），门内 cfg 恒被编译——gate-off 形态（默认 feature 不含 axum）只有这
# 里核到：error.rs 在无 axum 依赖图下必须独立成立，误在门外引用 axum 即红。
echo "▶ Rust gate-off 编译检查 (cargo check -p ledger-infra，默认 feature 不含 axum)"
( cd src-app && cargo check -p ledger-infra )

# 全仓无 tauri 守门（issue #2083 / ADR-0145 决策 4/7，#2074 零链接门升级）：
# Tauri 壳退役后全仓（workspace 全成员、全依赖种类）不得出现 tauri——
# 判定语义：tauri 不在任何成员依赖图 = cargo tree -i 报 did not match——grep
# 命中即绿；tauri 在图（要抓的回归）或其他 cargo 失败（陈旧锁等）grep 不中即红
#（fail-closed，单一命令两宿主同判：CI backend-lint 无 uv，故为命令型门）。
echo "▶ 全仓无 tauri 守门 (cargo tree --workspace -i tauri)"
( cd src-app && cargo tree --workspace -i tauri --edges normal 2>&1 | grep -q "did not match any packages" )

echo "▶ Rust 格式检查 (cargo fmt --all --check)"
( cd src-app && cargo fmt --all -- --check )

echo "▶ 基础设施 rustdoc 门禁 (cargo doc -p ledger-infra --no-deps, -D warnings)"
( cd src-app && RUSTDOCFLAGS='-D warnings' cargo doc -p ledger-infra --no-deps )

./scripts/check-docs.sh

echo "▶ 全仓无 tauri 守门 (uv run scripts/check_no_tauri.py)"
uv run scripts/check_no_tauri.py

echo "▶ 版本元数据一致性检查 (uv run scripts/check_versions.py)"
uv run scripts/check_versions.py

echo "▶ 结构守门检查 (uv run scripts/check_structure.py)"
uv run scripts/check_structure.py

echo "▶ 基础设施账本数据表 DML 禁令 (uv run scripts/check_infra_dml.py)"
uv run scripts/check_infra_dml.py

echo "▶ 东财行情面端点零残留源码扫描 (uv run scripts/check_eastmoney_residue.py)"
uv run scripts/check_eastmoney_residue.py

# 壳根包裸字号直调零残留守门（issue #2108 / spec #2099 / ADR-0147 决策 4）：
# 视图零裸值纪律源码扫描（DESIGN.md §7）——text_size(px(<数字字面量>)) 直调
# 即红，字号取值唯一来源 design::font_size 字阶。
echo "▶ 壳根包裸字号直调零残留扫描 (uv run scripts/check_bare_font_size.py)"
uv run scripts/check_bare_font_size.py

echo "▶ i18n key 全等检查 (uv run scripts/check_i18n_keys.py)"
uv run scripts/check_i18n_keys.py

echo "▶ Rust 测试守门检查 (uv run scripts/check_test_support.py)"
uv run scripts/check_test_support.py

echo "▶ 测试执行覆盖守门 (uv run scripts/test_exec.py check)"
uv run scripts/test_exec.py check

# 守门脚本测试（issue #1991 / 运行时迁移 #2174）：scripts/ 侧测试的唯一运行口径
# = uv run scripts/run_tests.py（pytest，PEP 723 内联依赖钉死版本）——测试经
# spawn 'uv run' 依赖 uv 在 PATH。挂载登记归 scripts/gate_mounts.py，删除本步即红。
echo "▶ 守门脚本测试 (uv run scripts/run_tests.py)"
uv run scripts/run_tests.py

echo "✅ 所有检查通过"
