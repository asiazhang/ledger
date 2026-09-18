#!/bin/sh
# 运行 Rust 测试（本地测试入口，issue #1112；e2e 执行器切 cargo-nextest，ticket
# #1496）：三条入口，一次跑完 workspace 全部成员的测试面。
#
# 入口① 并发（scripts/test-exec.ts）：一条 `cargo test --workspace --no-run` 构建
#   一次，再由执行器统一调度全部测试二进制——全局并行度 = min(CPU 数, 待跑二进制
#   数)，每个二进制固定 RUST_TEST_THREADS=1（只约束 libtest 线程），消掉「各二进制
#   各自开满 libtest 线程」的 CPU 超订与顺序执行的尾部空转；覆盖守门同址执行——本脚本
#   先跑 `bun scripts/test-exec.ts check` 核入口接线：新增测试目标不会被静默漏跑，
#   入口命令被 echo/注释/删除时立刻非零退出，不再静默退出 0 却一条测试都没跑
#   （#1112 第三轮审查 P1）。nextest 承接的 e2e 新目标不在本入口队列（否则重复跑）。
# 入口② e2e 新目标（cargo-nextest，ticket #1496 / spec #1494）：rstest-bdd 目标
#   经 nextest **进程级 per-test** 调度——每个 Scenario 一个进程、并行度 = CPU 数；
#   进程内 libtest 线程并行对世界构造是负收益（原因与实测见 spec #1494）。
#   超时/重试/并行度配置住 `src-tauri/.config/nextest.toml`。
# 入口③ 非并发（cargo 自有 runner，两条命令）：e2e 旧目标（cucumber，
#   `harness = false` 自定义 runner，nextest 不能列举）与 doc-test 不纳并发调度，
#   仍由 cargo 驱动；收口票 #1508 删除 cucumber 后入口② 与入口③ 的 e2e 线合流。
#
# 显式 --workspace：非虚拟 workspace 下 cargo 默认只作用于根包，缺范围参数会
# 静默漏跑成员 crate（spec #1086 / issue #1087）。
set -eu
cd "$(dirname "$0")/.."

# 并发执行器以 Bun 运行时执行（issue #734 / ADR-0083）：缺失即显式报错，不静默降级
if ! command -v bun >/dev/null 2>&1; then
  echo "✗ 未检测到 bun：测试执行器以 Bun 运行时执行（ADR-0083），请安装 bun（CI 固定 1.4.0）后重试" >&2
  exit 1
fi

# e2e 新目标经 cargo-nextest 调度（ticket #1496）：缺失即显式报错，不静默降级为
# 「少跑一个目标」；CI 由 backend job 固定版本安装。
if ! command -v cargo-nextest >/dev/null 2>&1; then
  echo "✗ 未检测到 cargo-nextest：e2e 新目标经 nextest 进程级 per-test 调度（ticket #1496，配置见 src-tauri/.config/nextest.toml），请安装后重试" >&2
  exit 1
fi

# 入口自检先行：本脚本三条命令的接线是覆盖守门的核对对象，停用即红
bun scripts/test-exec.ts check

# 入口①：并发执行单元测试与集成测试（构建一次 + 全二进制并发，nextest/e2e 除外）
bun scripts/test-exec.ts

# 入口②：e2e 新目标（rstest-bdd）经 nextest 进程级 per-test 调度
( cd src-tauri && cargo nextest run --workspace --test e2e_rstest )

# 入口③ 非并发：e2e 旧目标（cucumber，harness = false 自定义 runner，nextest 不能列举）
( cd src-tauri && cargo test --workspace --test e2e )

# 入口③ 非并发：doc-test 由 rustdoc 生成测试二进制，不进并发执行器，仍由 cargo 驱动
( cd src-tauri && cargo test --workspace --doc )
