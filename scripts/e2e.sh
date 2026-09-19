#!/bin/sh
# e2e 一条命令入口（ticket #1496 / spec #1494；收口票 #1508）：全部 `.feature`
# 场景在新目标（rstest-bdd，`tests/e2e_rstest.rs`）经 cargo-nextest **进程级
# per-test** 调度——每个 Scenario 一个进程、并行度 = CPU 数；超时/重试/并行度
# 配置住 `src-tauri/.config/nextest.toml`（进程内 libtest 线程并行对世界构造是
# 负收益，原因与实测见 ADR-0129）。
#
# 旧 cucumber 目标（`tests/e2e.rs`，`harness = false`）与 cucumber 依赖已随 #1508
# 删除：本脚本与 CI backend job 各自只剩这一条 nextest 命令（「一条 nextest 入口
# 跑完 e2e」），不再有「nextest + cargo 双轨」。
#
# 场景数全等守门（#1508 AC）：`scenarios!` 生成的场景测试数必须等于 feature 的
# `Scenario:` 行数——删掉 `scenarios!` 绑定（或漏绑一份 feature）即红，防「绑定面
# 静默缩减而测试仍绿」（覆盖守门 scripts/check-e2e-step-coverage.ts 管「步骤
# 注册面」，本守门管「场景 → 测试」的一一对应）。
#
# scripts/test.sh 复用本脚本（三条入口里的 e2e 段）。
#
# 显式 --workspace：非虚拟 workspace 下 cargo 默认只作用于根包（spec #1086）。
set -eu
cd "$(dirname "$0")/.."

# e2e 经 cargo-nextest 调度：缺失即显式报错，不静默降级；CI 由 backend job
# 固定版本安装（.github/workflows/build.yml backend job）。
if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "✗ 未检测到 cargo-nextest：e2e 经 nextest 进程级 per-test 调度（ticket #1496，配置见 src-tauri/.config/nextest.toml），请安装后重试" >&2
  exit 1
fi

# 场景数全等守门：nextest 列举的场景测试数（`e2e_rstest scenarios::` 前缀的测试名，
# 不含运行时注册表断言）⇔ feature 的 `Scenario:` 行数。
if ! e2e_test_list=$( cd src-tauri && cargo nextest list --workspace --test e2e_rstest ); then
  echo "✗ 场景数全等守门：cargo nextest list 失败（e2e 目标无法列举）" >&2
  exit 1
fi
scenario_tests=$( printf '%s\n' "$e2e_test_list" | grep -c 'e2e_rstest scenarios::' || true )
feature_scenarios=$( grep -hE '^[[:space:]]*Scenario:' src-tauri/tests/e2e/features/*.feature | wc -l | tr -d ' ' )
if [ "$scenario_tests" -ne "$feature_scenarios" ]; then
  echo "✗ 场景数全等守门：生成的场景测试 $scenario_tests 个 ≠ feature 场景 $feature_scenarios 条（绑定面与 feature 不一致：删/漏 scenarios! 绑定即红）" >&2
  exit 1
fi
echo "✓ 场景数全等守门：场景测试 $scenario_tests 个 = feature 场景 $feature_scenarios 条"

# 唯一 e2e 命令：全部场景经 cargo-nextest 进程级 per-test 调度
( cd src-tauri && cargo nextest run --workspace --test e2e_rstest )
