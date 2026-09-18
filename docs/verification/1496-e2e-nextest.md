# e2e 测试执行器切 cargo-nextest（issue #1496 / 父 spec #1494）

> 本票把 e2e 执行面从「同一进程内 libtest 线程并行」切到 **cargo-nextest 的进程级
> per-test 调度**：新增的 rstest-bdd e2e 目标每个 Scenario 一个进程、并行度按 CPU
> 收敛；旧 cucumber 目标（`harness = false` 自定义 runner）不支持 nextest 依赖的
> `--list --format terse` 协议，进显式排除名单后仍由 cargo 自有入口执行——两个 e2e
> 目标都在覆盖守门的调度面内，任一处漏跑即红。本文件记录口径、实测数字、负向证据、
> 范围外修复与已知边界。

## 交付物

- `src-tauri/.config/nextest.toml`（新）：nextest 仓库配置——并行度（`test-threads =
  "num-cpus"`）、失败不早停（`fail-fast = false`，与既有执行器「跑完全部才退出」
  同口径）、**不重试**（`retries = 0`：红灯必须可直接复现，重试只会把 flaky 藏起来）、
  单场景超时（`slow-timeout = { period = "60s", terminate-after = 2 }`：世界构造卡死
  /死锁不得拖死本地与 CI，旧 cucumber 单进程无此保护）、整轮兜底
  （`global-timeout = "15m"`）、以及自定义 harness 的排除名单
  （`default-filter = "not binary(e2e)"`）。
- `scripts/test.sh`（改）：e2e 新目标改经 `cargo nextest run --workspace --test
  e2e_rstest`（进程级 per-test）；缺失 `cargo-nextest` 显式报错不静默降级；旧
  cucumber 目标与 doc-test 的非并发入口不变。三条入口一次跑完 workspace 全部测试面。
- `scripts/test-exec.ts`（改）：覆盖守门新增 nextest 承接面——`NEXTEST_TARGETS` 登记
  ⇔ `scripts/test.sh` 的 `cargo nextest run … --test <name>` 名单双向全等；并按
  `src-tauri/.config/nextest.toml` 的 `default-filter` 排除名单 ⇔ `harness = false`
  目标清单双向全等核对；并发入口队列剔除 nextest 承接目标（不重复跑）。
- `scripts/check-structure.ts`（改）：workspace 范围门禁把两词命令 `cargo nextest
  run` 与 `cargo clippy/test/fmt` 同待遇（缺 `--workspace` 即红；`cargo nextest
  --version` 这类非运行命令不在列）。
- `.github/workflows/build.yml`（改）：backend job 安装固定版本 cargo-nextest 并改用
  `cargo nextest run --workspace --lib --test '*'` 执行测试，旧 cucumber 目标单独一条
  `cargo test --workspace --test e2e`。
- 夹具与接线锁：`scripts/test-exec.test.ts` 增 5 条 nextest 面负向用例（29/29 全绿）、
  `scripts/check-structure.test.ts` 增 2 条 workspace 覆盖用例（228/228 全绿）。

## 执行面与三入口口径

唯一权威 = `bun scripts/test-exec.ts plan` 输出（本票落点：**53 个目标 = 并发入口 32
⊎ nextest 入口 1（`tauri-app::e2e_rstest`）⊎ 非并发入口 20（cucumber `e2e` +
doc-test 19）**）：

1. **并发入口**（`bun scripts/test-exec.ts`）：lib 单测 + bin 单测 + 其余集成测试，
   沿用「构建一次 + 全二进制并发 + 每二进制 `RUST_TEST_THREADS=1`」。
2. **nextest 入口**（`cargo nextest run --workspace --test e2e_rstest`）：e2e 新目标
   的 12 个 Scenario 各占一个进程，并行度 = CPU 数（配置单点）。
3. **非并发入口**（cargo 自有 runner）：旧 cucumber e2e（自定义 harness，nextest
   不能列举）+ doc-test。

「本地一条命令跑完全部 e2e」= `./scripts/test.sh`（同时跑另外两条入口）；只跑 e2e
时是它里面的两条命令（nextest 一条 + cucumber 一条）。收口票 #1508 删除 cucumber
后 2 与 3 的 e2e 线合流为 nextest 单入口，`default-filter` 随之下线。

## 实测（本机 macOS arm64 / 12 CPU / cargo 1.98 + kache，构建全热）

**e2e 新目标（`--workspace --test e2e_rstest`，12 个 accounts.feature 场景）**，每组
3 次，`/usr/bin/time -p` 口径；`nextest Summary` 是测试执行窗口，`real/user/sys` 含
nextest + cargo 启动（约 0.5s 固定开销，故命令级比值被摊薄）：

| `--test-threads` | nextest Summary | real | user | sys | 命令级 CPU/墙钟 |
| --- | --- | --- | --- | --- | --- |
| 1（串行对照） | 0.734–0.757s | 1.26–1.28s | 1.00s | 0.24–0.26s | ≈0.99 |
| 4（CI 4 vCPU 画像） | 0.193–0.194s | 0.72s | 1.02–1.03s | 0.24–0.25s | ≈1.76 |
| 12（默认 = CPU 数） | 0.108–0.115s | 0.63–0.64s | 1.28–1.29s | 0.27s | ≈2.46 |

测试窗口的串/并加速：12 线程 **6.8×**、4 线程 **3.8×**（1 与 12 线程对照，同一批
场景）；进程级并行成立，与 spec #1494 spike（0.73s → 0.11s）同量级。命令级 CPU/墙钟
在 12 线程下 ≥2（spec user story 2 的判据），4 线程下 1.76 是「12 个 ~15ms 场景 +
0.5s 固定启动」的口径摊薄，不是串行残留。

**CI 口径命令（`cargo nextest run --workspace --lib --test '*'`）**：1763 tests /
1763 passed / 4 skipped / 0 failed，`Summary 17.269s`、`real 17.97s`、`user 100.80s`、
`sys 14.78s` → 命令级 CPU/墙钟 **6.4**（12 CPU 机器）。注：本口径下 e2e 迁移子集只
占 12 个 Scenario，旧 cucumber 目标仍由第二条命令执行；整段 e2e 时长要等逐域迁移
（#1498–#1507）把场景全部并入新目标后才有对照意义。

**旧目标行为不变**：`cargo test --workspace --test e2e` 仍报 `453 scenarios (453
passed) / 3139 steps (3139 passed)`（与 #1112 记录的历史数字逐数一致，场景数不减，
迁移期双轨）。

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
  `global-timeout = 15m`、`fail-fast = false`（一次看全部失败）——见交付物。
- **CI e2e 段时长**：迁移子集口径即上表 `--test-threads 4` 行（12 场景
  0.193–0.194s）；backend job 的端到端时长与 e2e 段由本次 PR 运行的 CI 记录回填
  （见「未验证项」）。

## 负向验收（删除即变红）

夹具用例（`pnpm exec vitest run scripts/test-exec.test.ts` → 29/29）逐条对准一条可观察
失败输出；下面三条在**真实仓库文件**上实测（制造变红 → 恢复 → 变绿，
观察面 = `bun scripts/test-exec.ts check` 的退出码与输出）：

| 制造方式 | 观测 |
| --- | --- |
| 删 `scripts/test.sh` 的 nextest 运行行 | 退出 1：`tauri-app::e2e_rstest 登记为 nextest 承接，但 scripts/test.sh 无 …--test e2e_rstest——nextest 入口被删/改写即红` |
| `nextest.toml` 的 `default-filter` 多排一个 libtest 目标（`api_server`） | 退出 1：`default-filter 排除了 api_server，但它不是 harness = false 目标——该目标会被 nextest 静默漏跑` |
| 删 `nextest.toml` 的 `default-filter` 行 | 退出 1：`缺 default-filter——自定义 harness 目标（e2e）会被 nextest 纳入列举并失败` |
| 恢复原状 | 退出 0：`目标 53 个 = 并发入口 32 ⊎ nextest 入口 1 ⊎ 非并发入口 20` |

夹具侧另有：nextest 入口登记拼写漂移（`--test e2e_rstst`）→ 红、
`default-filter` 出现未取反的 `binary(api)` → 红；`scripts/check-structure.ts` 侧新增
`cargo nextest run` 缺 `--workspace` → 红、带 `--workspace`/`cargo nextest --version`
→ 不假红两条。

**nextest 与自定义 harness 的硬边界（实测）**：`harness = false` 目标不支持
`--list --format terse`，nextest 列举即 `error: creating test list failed`——故旧
cucumber 目标只能排除后走 cargo；新增自定义 harness 目标漏进排除名单会让 `cargo
nextest run` fail loud（不是静默漏跑），覆盖守门也同时点名（上表第 3 行）。

## 既有测试面不变（改后实测：`./scripts/test.sh` 一次跑完，退出码 0）

| 入口 | 实测 |
| --- | --- |
| 并发入口（32 个二进制） | 32/32 通过，1787 passed / 0 failed / 4 ignored；墙钟 25.32s（并行度 12），各二进制耗时之和 119.38s，加速 4.71× |
| nextest 入口（e2e 新目标） | 12 tests run / 12 passed / 0 skipped（同一批 12 个 Scenario，改前由并发入口的进程内串行跑） |
| 非并发入口 · 旧 cucumber e2e | 453 scenarios (453 passed) / 3139 steps (3139 passed) |
| 非并发入口 · doc-test | 4 passed / 1 ignored |

迁移是**执行面替换**：12 个账户域 Scenario 从「并发入口里的一个二进制（进程内串行）」
挪到 nextest 入口（进程级 per-test），其余测试面逐项不变；`./scripts/test.sh` 端到端
墙钟 60.5s（本机口径，含 453 场景 cucumber 与 doc-test）。

## 范围外修复（单独提交）

`src-tauri/tests/commands/sync_channel.rs` 的 `fresh_app`（本套件自建 mock 应用的
接线单点）注册了余额刷新与交易六向钩子，却漏了**写后即时同步钩子**
（`ledger_sync_engine::trigger::install_after_write_hook`）。该钩子是进程级单例
（`OnceLock`），本套件又不得经测试建库工厂（ADR-0084 决策 3），于是：

- 全二进制同跑（libtest 单进程多线程）时，由别的测试经测试工厂装上 → `sync_trigger::
  write_entry_enqueues_upload_via_scheduler` 通过；
- 单测隔离运行（`cargo test -- --exact` 或 nextest 进程级 per-test）时永不装上 →
  写 op 不投递信号 → 轮次不触发 → 10s 超时假红。

实测（本机，同一 commit）：`cargo test --test commands`（29 例同进程）29 passed；
`cargo nextest run --test commands -E 'test(write_entry_enqueues_upload_via_scheduler)'`
0 passed / 1 failed（10.14s 超时）——**进程级隔离正是 nextest 的结构性收益，本票把它
暴露的既有进程状态依赖就地修掉**（补一行显式钩子登记，与测试工厂同形）。修复后同
命令 1 passed（0.27s）。属「用户可见缺陷」（单测假红）类，单独提交并写明根因。

## 未验证项

- **CI 实测数字待回填**：本机无 CI runner 画像，上表为 macOS arm64 / 12 CPU 口径；
  backend job 的实际 e2e 段时长与 job 端到端耗时在本次 PR 的 CI 运行里记录（merged
  前的 `:latest` 镜像 + job 内安装路径已按真机 CI 接线写死，故该运行即是安装/缓存
  策略的端到端验证）。
- **CI 镜像重建后的零安装形态未验证**：若后续把 nextest 收编进
  `backend-ci.Dockerfile`（候选优化，非本票范围），需在 PR 分支手动 dispatch
  `ci-image.yml` 发布验证，本票不动共享镜像。
- **全量 e2e 的 nextest 收益未验证**：当前只有 accounts.feature（12/453 场景）在
  nextest 调度面内，整段 e2e 的时长/比值要等逐域迁移（#1498–#1507）完成后才有对照
  意义；收口票 #1508 记录对照数字（目标：CI e2e 65.6s → ≤30s、CPU/墙钟 ≥2）。
- **Windows / Linux 上的 nextest 行为未验证**（本机 macOS）；CI 侧由本 PR 运行覆盖
  （backend job 跑在 Linux 容器内）。
