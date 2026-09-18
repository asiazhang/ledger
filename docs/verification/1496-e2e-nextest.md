# e2e 测试执行器切 cargo-nextest（issue #1496 / 父 spec #1494）

> 本票把 e2e 执行面从「同一进程内 libtest 线程并行」切到 **cargo-nextest 的进程级
> per-test 调度**：新增的 rstest-bdd e2e 目标每个 Scenario 一个进程、并行度按 CPU
> 收敛；旧 cucumber 目标（`harness = false` 自定义 runner）不支持 nextest 依赖的
> `--list --format terse` 协议，进显式排除名单后仍由 cargo 自有入口执行——两个 e2e
> 目标都在覆盖守门的调度面内，任一处漏跑即红。本文件记录口径、实测数字、负向证据、
> 评审补强、范围外修复与已知边界。

## 交付物

- `scripts/e2e.sh`（新）：**本地一条命令跑完全部 e2e**（新目标 nextest 进程级
  per-test + 旧 cucumber 目标 cargo 自有 runner）；缺 `cargo-nextest` 显式报错，
  不静默降级为「少跑一个目标」。
- `src-tauri/.config/nextest.toml`（新）：nextest 仓库配置——并行度（`test-threads =
  "num-cpus"`）、失败不早停（`fail-fast = false`，与既有执行器「跑完全部才退出」
  同口径）、**不重试**（`retries = 0`：红灯必须可直接复现，重试只会把 flaky 藏起来）、
  单场景超时（`slow-timeout = { period = "60s", terminate-after = 2 }`：世界构造卡死
  /死锁不得拖死本地与 CI，旧 cucumber 单进程无此保护）、整轮兜底
  （`global-timeout = "15m"`）、以及自定义 harness 的排除名单
  （`default-filter = "not binary(e2e)"`）。
- `scripts/test.sh`（改）：三条入口——并发执行器、`./scripts/e2e.sh`（全部 e2e）、
  doc-test；一次跑完 workspace 全部测试面。
- `scripts/test-exec.ts`（改）：覆盖守门新增 nextest 承接面与 CI 测试执行面（见
  「执行面与守门口径」），并把 `harness = false` 接线判据的宿主从 `scripts/test.sh`
  移到 `scripts/e2e.sh`（e2e 接线单一宿主）。
- `scripts/check-structure.ts`（改）：workspace 范围门禁把两词命令 `cargo nextest
  run` 与 `cargo clippy/test/fmt` 同待遇（缺 `--workspace` 即红；`cargo nextest
  --version` 这类非运行命令不在列）；命令词表单一来源（数组形态与逐行形态同源）。
- `.github/workflows/build.yml`（改）：backend job 安装固定版本 cargo-nextest 并改用
  `cargo nextest run --workspace --lib --test '*'` 执行测试，旧 cucumber 目标单独一条
  `cargo test --workspace --test e2e`。
- 夹具与接线锁：`scripts/test-exec.test.ts` 34/34（含 8 条 nextest/CI 面新用例）、
  `scripts/check-structure.test.ts` 228/228（含 2 条 `cargo nextest run` 用例）。

## 执行面与守门口径

唯一权威 = `bun scripts/test-exec.ts plan` 输出（本票落点：**53 个目标 = 并发入口 32
⊎ nextest 入口 1（`tauri-app::e2e_rstest`）⊎ 非并发入口 20（cucumber `e2e` +
doc-test 19）**）：

1. **并发入口**（`bun scripts/test-exec.ts`）：lib 单测 + bin 单测 + 其余集成测试，
   沿用「构建一次 + 全二进制并发 + 每二进制 `RUST_TEST_THREADS=1`」。
2. **nextest 入口**（`scripts/e2e.sh` 的 `cargo nextest run --workspace --test
   e2e_rstest`）：e2e 新目标的 12 个 Scenario 各占一个进程，并行度 = CPU 数。
3. **非并发入口**：`scripts/e2e.sh` 的旧 cucumber 目标（自定义 harness，nextest
   不能列举）+ `scripts/test.sh` 的 doc-test。

**本地一条命令**：`./scripts/e2e.sh` 跑完全部 e2e；`./scripts/test.sh` 跑全部测试面
（其中 e2e 段就是调用前者）。收口票 #1508 删除 cucumber 后 2 与 3 的 e2e 线合流为
nextest 单入口，`default-filter` 随之下线。

**覆盖守门（`bun scripts/test-exec.ts check`，挂 `scripts/check.sh` + CI frontend
job）**——本票新增/改动的四条：

- nextest 承接登记（`NEXTEST_TARGETS`，唯一声明处）⇔ `scripts/e2e.sh` 的
  `cargo nextest run … --test <name>` 名单**双向全等**；`scripts/test.sh` 必须调用
  `./scripts/e2e.sh`。删 nextest 运行行即红——**不静默降级回并发入口**（那会退回
  进程内单线程串行，正是本票要消灭的形态）。
- nextest 配置的 `default-filter` **只允许**「`not binary(<自定义 harness 目标名>)`
  子句 + `and`」，且排除名单与 `harness = false` 目标清单双向全等；逐名回查清单里
  **所有同名目标**（nextest 的 `binary()` 按二进制名匹配、与包无关，单测名取 crate
  名）——多排一个名字、写 `test(...)` 之类别的子句、或名称同时命中 `harness = true`
  目标，都按红处置（否则那些测试被静默移出 nextest 调度面）。
- CI 测试执行面：CI workflow 必须有 `cargo nextest run …`，且每个 `harness = false`
  目标必须有一条 `cargo test --workspace --test <name>`——CI 侧删任一条即静默减面。
- `harness = false` 声明集合 ⇔ `scripts/e2e.sh` 的 `--test <name>` 名单（宿自主
  `scripts/test.sh` 迁到 `scripts/e2e.sh`，判据与双向全等语义不变）。

## 实测（本机 macOS arm64 / 12 CPU / cargo 1.98 + kache，构建全热）

**e2e 新目标（`--workspace --test e2e_rstest`，12 个 accounts.feature 场景）**，每组
3 次，`/usr/bin/time -p` 口径；`nextest Summary` 是测试执行窗口，`real/user/sys` 含
nextest + cargo 启动（约 0.5s 固定开销，故命令级比值被摊薄）：

| `--test-threads` | nextest Summary | real | user | sys | 命令级 CPU/墙钟 |
| --- | --- | --- | --- | --- | --- |
| 1（串行对照） | 0.734–0.757s | 1.26–1.28s | 1.00s | 0.24–0.26s | ≈0.99 |
| 4（CI 4 vCPU 画像） | 0.193–0.194s | 0.72s | 1.02–1.03s | 0.24–0.25s | ≈1.76 |
| 12（默认 = CPU 数） | 0.108–0.115s | 0.63–0.64s | 1.28–1.29s | 0.27s | ≈2.46 |

测试窗口的串/并加速：12 线程 **6.8×**、4 线程 **3.8×**（同一批场景）；进程级并行成立，
与 spec #1494 spike（0.73s → 0.11s）同量级。

**CI 口径命令（`cargo nextest run --workspace --lib --test '*'`，本机）**：
1763 tests / 1763 passed / 4 skipped / 0 failed，`Summary 17.269s`、`real 17.97s`、
`user 100.80s`、`sys 14.78s` → 命令级 CPU/墙钟 **6.4**（12 CPU 机器）。

**`./scripts/test.sh` 端到端**：退出码 0，墙钟 58.6s（含 453 场景 cucumber）——

| 入口 | 实测 |
| --- | --- |
| 并发入口（32 个二进制） | 32/32 通过，1787 passed / 0 failed / 4 ignored；墙钟 23.74s（并行度 12），各二进制耗时之和 112.33s，加速 4.73× |
| nextest 入口（e2e 新目标） | 12 tests run / 12 passed / 0 skipped（同一批 12 个 Scenario，改前由并发入口的进程内串行跑） |
| 非并发入口 · 旧 cucumber e2e | 453 scenarios (453 passed) / 3139 steps (3139 passed) |
| 非并发入口 · doc-test | 4 passed / 1 ignored |

迁移是**执行面替换**：12 个账户域 Scenario 从「并发入口里的一个二进制（进程内串行）」
挪到 nextest 入口（进程级 per-test），其余测试面逐项不变。

## CI 接线（backend job）

- **nextest 可用性**：job 内 `run:` 步骤下载 nextest **0.9.145** 预编译二进制
  （`x86_64-unknown-linux-musl`，静态链接、不耦合镜像 glibc），校验 sha256
  （`cd3c8519…8883`）后解到 `$CARGO_HOME/bin`，并 `cargo nextest --version` 打印版本。
  运行处在既有 `ghcr.io/asiazhang/ledger-ci-backend` 容器内（`LANG=C.UTF-8`、mold、
  rust-cache 都不变）。
- **刻意不烘进 CI 镜像**：镜像只在 merge 后按 paths 触发重建（`ci-image.yml`），PR CI
  挂的仍是旧镜像，烘入会让本 PR 的 backend job 直接 `no such command`；nextest 与前端
  job 的 bun（`setup-bun`）同属「in-job 测试工具」，不是 apt 系统依赖（#1109 的镜像
  路径与「job 内零 apt 安装」不变）。
- **缓存策略不变**：nextest 经 cargo 构建、复用同一 `src-tauri/target`，rust-cache 的
  restore/save key 与 `CARGO_PROFILE_DEV_DEBUG=0` / `RUSTFLAGS=-C link-arg=-fuse-ld=mold`
  均不受影响（nextest 自身的元数据住 `target/nextest/`，不参与指纹）。
- **并行度**：CI runner 4 vCPU ⇒ `test-threads = "num-cpus"` = **4**（本机 12）。
- **超时/重试**：`retries = 0`（红必须可复现）、`slow-timeout = 60s × 2`、
  `global-timeout = 15m`、`fail-fast = false`（一次看全部失败）。
- **CI 实测（本 PR 的 backend job，run 35363456259 / 4 vCPU 容器，job 105660097579）**：
  见下节「CI 运行回填（本 PR 实测）」。

## CI 运行回填（本 PR 实测）

run `35363456259`（backend job 105660097579，ubuntu-latest 4 vCPU + 后端 CI 镜像；
nextest 0.9.145，2026-09-18 15:36:32Z → 15:42:52Z，job 380s）：

| 步骤 | 时间窗 | 结论 |
| --- | --- | --- |
| 安装 cargo-nextest（固定版本） | 15:37:52（可见工作 ≈0.6s：curl → `sha256sum -c` OK → 解压 → 版本打印） | `cargo-nextest 0.9.145 (00af4550e 2026-09-16)`——**容器内可用性已实测** |
| Rust 单元测试 + 集成测试（cargo-nextest） | 15:37:52 → 15:40:50（178s，含缓存恢复后的编译；测试执行窗口 `Summary 79.397s`） | `1763 tests run: 1763 passed, 4 skipped`；`Starting 1763 tests across 31 binaries (… 1 binary skipped via profile.default.default-filter)`——**default-filter 在 CI 上确实把 cucumber 二进制排除在列举之外** |
| ↳ e2e 新目标（迁移子集） | 12 个 `tauri-app::e2e_rstest` PASS 落在 15:40:45.4–15:40:46.6（**1.2s**，单个 0.19–0.21s） | 进程级 per-test、并行度 = 4（runner vCPU 数） |
| 旧 cucumber e2e 目标 | 15:40:50 → 15:42:47（117s） | `451 scenarios (451 passed) / 3122 steps (3122 passed)`（CI 口径 451，与本机 453 的既有差异一致；本票不改旧目标） |

**缓存策略**：rust-cache 恢复 15:37:04 → 15:37:52（48s），`prefix-key: ci-image` 与
`save-if: main` 未动；nextest 复用同一 `target/`，安装步骤独立于缓存（每次 job 约
0.6s 可见工作）。**CI 侧无「nextest 装不上 / 列举失败 / 缓存失效」问题**。

## 负向验收（删除即变红）

夹具用例（`pnpm exec vitest run scripts/test-exec.test.ts` → 34/34）逐条对准一条可观察
失败输出；下面在**真实仓库文件**上实测（制造变红 → 恢复 → 变绿，观察面 =
`bun scripts/test-exec.ts check` 的退出码与输出）：

| 制造方式 | 观测 |
| --- | --- |
| 删 `scripts/e2e.sh` 的 nextest 运行行 | 退出 1：`tauri-app::e2e_rstest 登记为 nextest 承接，但 scripts/e2e.sh 无 …--test e2e_rstest——nextest 入口被删/改写即红` |
| `nextest.toml` 的 `default-filter` 多排一个 libtest 目标（`api_server`） | 退出 1：`default-filter 排除了 api_server：同名目标 tauri-app::api_server 不是 harness = false——它的测试会被静默移出 nextest 调度面` |
| 删 `nextest.toml` 的 `default-filter` 行 | 退出 1：`缺 default-filter——自定义 harness 目标（e2e）会被 nextest 纳入列举并失败` |
| `default-filter` 写别的子句（`… and not test(accounts)`） | 退出 1：`含未识别子句（余：nottestaccounts）——默认过滤器只允许 not binary(<…>) 与 and` |
| 删 `scripts/test.sh` 的 `./scripts/e2e.sh` 调用 | 退出 1：`scripts/test.sh 未调用 e2e 入口脚本——全部 e2e 目标未接入本地测试入口` |
| 删 CI 的 `cargo nextest run …` / 旧 cucumber 步骤 | 退出 1：`未切到 nextest` / `CI 侧漏跑即静默减面` |
| 恢复原状 | 退出 0：`目标 53 个 = 并发入口 32 ⊎ nextest 入口 1 ⊎ 非并发入口 20` |

**nextest 与自定义 harness 的硬边界（实测）**：`harness = false` 目标不支持
`--list --format terse`，nextest 列举即 `error: creating test list failed`——故旧
cucumber 目标只能排除后走 cargo；新增自定义 harness 目标漏进排除名单会让 `cargo
nextest run` fail loud（不是静默漏跑），覆盖守门也同时点名。

## 范围外修复（单独提交 2641418c）

`src-tauri/tests/commands/sync_channel.rs` 的 `fresh_app`（本套件自建 mock 应用的
接线单点）注册了余额刷新与交易六向钩子，却漏了**写后即时同步钩子**
（`ledger_sync_engine::trigger::install_after_write_hook`）。该钩子是进程级单例
（`OnceLock`），本套件又不得经测试建库工厂（ADR-0084 决策 3），于是：

- 全二进制同跑（libtest 单进程多线程）时，由别的测试经测试工厂装上 →
  `sync_trigger::write_entry_enqueues_upload_via_scheduler` 通过；
- 单测隔离运行（`cargo test -- --exact` 或 nextest 进程级 per-test）时永不装上 →
  写 op 不投递信号 → 轮次不触发 → 10s 超时假红。

实测（同一 commit）：`cargo test --test commands`（29 例同进程）29 passed；
`cargo nextest run --test commands -E 'test(write_entry_enqueues_upload_via_scheduler)'`
0 passed / 1 failed（10.14s 超时）；补一行显式登记后 1 passed（0.27s）。
**进程级隔离正是 nextest 的结构性收益**，本票把它暴露的既有进程状态依赖就地修掉
（与测试工厂同形）。属「用户可见缺陷」（单测假红）类，单独提交并写明根因。

## 已知边界（fail loud，不静默放过）

- **spec #1494 US2「e2e CPU/墙钟比 ≥2」本票只对迁移子集成立**：12 个 Scenario 在
  12 线程下命令级比值 2.46（达标），4 线程画像 1.76——后者是「12 个 ~15ms 场景 +
  0.5s 固定启动」的摊薄，测试窗口本身的加速是 3.8×。整段 e2e 的比值判据要等逐域
  迁移（#1498–#1507）把场景全部并入新目标，由收口票 #1508 记录（目标 CI e2e
  65.6s → ≤30s、CPU/墙钟 ≥2）。
- **spec #1494 US18「本地与 CI 同一套测试执行入口」本票未完全兑现**：本地非 e2e 面仍
  走 `scripts/test-exec.ts` 自建并发执行器（CI 不跑该入口），切到 nextest 单入口是
  #1508 的验收项；本票只把两条 e2e 入口在本地与 CI 对齐（nextest 跑 libtest 目标、
  cucumber 走 cargo 自有 runner）。
- **CI 镜像重建后的零安装形态未验证**：若后续把 nextest 收编进
  `backend-ci.Dockerfile`（候选优化，非本票范围），需在 PR 分支手动 dispatch
  `ci-image.yml` 发布验证，本票不动共享镜像。
- **Windows / Linux 上的 nextest 行为未在本机验证**（本机 macOS）；CI 侧由本 PR 运行
  覆盖（backend job 跑在 Linux 容器内）。
