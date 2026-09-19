#!/bin/sh
# e2e 一条命令入口（ticket #1496 / spec #1494）：跑完全部两个 e2e 目标——
#   ① 新目标（rstest-bdd，`tests/e2e_rstest.rs`）：cargo-nextest **进程级 per-test**
#      调度，每个 Scenario 一个进程、并行度 = CPU 数；超时/重试/并行度配置住
#      `src-tauri/.config/nextest.toml`（进程内 libtest 线程并行对世界构造是负收益，
#      原因与实测见 spec #1494）。
#   ② 旧目标（cucumber，`tests/e2e.rs`，`harness = false` 自定义 runner）：不支持
#      nextest 依赖的 `--list --format terse` 协议，由 nextest 配置的 default-filter
#      排除后仍走 cargo 自有 runner；收口票 #1508 删除 cucumber 后本行随之下线。
#
# scripts/test.sh 复用本脚本（三条入口里的 e2e 段），覆盖守门（scripts/test-exec.ts）
# 读取本文件的命令行：`harness = false` 目标清单 ⇔ ② 的 `--test <name>` 名单、
# NEXTEST_TARGETS 登记 ⇔ ① 的 `--test <name>` 名单，两个方向都双向全等——删任一行
# 即红（入口②被删不静默降级回进程内串行）。
#
# 显式 --workspace：非虚拟 workspace 下 cargo 默认只作用于根包（spec #1086）。
set -eu
cd "$(dirname "$0")/.."

# e2e 新目标经 cargo-nextest 调度（ticket #1496）：缺失即显式报错，不静默降级为
# 「少跑一个目标」；CI 由 backend job 固定版本安装（build.yml backend job）。
if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "✗ 未检测到 cargo-nextest：e2e 新目标经 nextest 进程级 per-test 调度（ticket #1496，配置见 src-tauri/.config/nextest.toml），请安装后重试" >&2
  exit 1
fi

# ① e2e 新目标（rstest-bdd）经 nextest 进程级 per-test 调度
( cd src-tauri && cargo nextest run --workspace --test e2e_rstest )

# ② e2e 旧目标（cucumber，harness = false 自定义 runner，nextest 不能列举）
( cd src-tauri && cargo test --workspace --test e2e )
