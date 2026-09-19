# ADR-0129: 后端 e2e BDD 运行器切换——rstest-bdd 场景绑定 + cargo-nextest 进程级 per-test

- 状态：已接受（spec #1494 定稿，收口票 #1508；实现已随 #1495–#1508 落地）
- 日期：2026-09-19
- 作者：Ledger 项目
- 关联：ADR-0086（BDD e2e 步骤共享层加深——步骤输入工厂 / 步骤动词 / 快照分组纪律切换后原样保留）；ADR-0087（测试三层权威层——e2e 层定位与断言归属不变）；ADR-0084（统一测试数据库工厂——与 BDD 层互不消费的分层边界不变）；ADR-0083（守门脚本 Bun 运行时——覆盖守门的运行时载体）；词汇表：测试基础设施域 测试三层与权威层、测试世界（World）、步骤动词、快照分组、断言强度

## 背景

后端 e2e BDD 原以 cucumber-rs 承载。两个缺陷在 spec #1494 的探针里被钉死：

1. **名义并发 64 实为单核串行**。cucumber-rs 的「并发」是协作式——全部 Concurrent 场景由同一个 task 的 `FuturesUnordered` 轮询，而本仓步骤体几乎全是同步阻塞调用，场景 future 一旦被 poll 就一口气跑完、不存在让出点。实测（macOS arm64 12 核）`32.08 real / 27.33 user / 1.38 sys`，CPU/墙钟 ≈ 0.9；调 `-c`、调并发场景上限、调 worker 数都不产生 CPU 并行。
2. **场景失败不置红**。自建 harness 以 `filter_run(...).await;` 丢弃返回值、未走 `run_and_exit`，失败可能被静默吞掉。

维护者视角：CI 关键路径被这一段吃掉；「并行暴露调度类缺陷」的目标达不到；失败不可靠。

## 决策

1. **运行器换 rstest-bdd（+ rstest）**：每个 Scenario 成为标准测试运行器里的独立 `#[test]`，并行、过滤、超时、重试、报告与失败置红全部回到标准测试运行器；`filter_run` 丢弃返回值那类缺陷在结构上不再可能。
2. **场景绑定是唯一新接缝**：`scenarios!` 递归扫描 e2e feature 目录自动生成绑定（不手写 N 个绑定函数、新增场景只改 `.feature`）；测试世界由 `#[fixture] fn world()` 单点提供，每场景一次 `LedgerWorld::new()`（各自独立内存库，场景间零状态交叉）。`#[scenario]` 只留给需要按 index/name/tags 精确控制的例外（当前仓零命中）。
3. **并行交 cargo-nextest 的进程级 per-test 调度**：进程内 libtest 线程并行对「世界构造（内存库 + 迁移）」是负收益（实测线程数增加而 sys 平方增长）；每个 Scenario 一个进程才是真并行，并天然消除进程级全局状态（自动备份偏好、行情熔断/节流、同步轮次闸门）的跨场景干扰。并行度 / 超时 / 重试是 `src-tauri/.config/nextest.toml` 的单点配置。
4. **单一 e2e 目标与单一入口**：e2e 只有一个目标（`tests/e2e_rstest.rs`），本地 `scripts/e2e.sh` 与 CI backend job 各自只有一条 `cargo nextest run --test e2e_rstest`——不做「nextest + cargo」长期双轨；旧 cucumber 目标（`harness = false`）与 cucumber 依赖随收口票 #1508 删除，`nextest.toml` 的 `default-filter` 排除名单一并下线。
5. **既有测试纪律原样保留（纯机械替换）**：步骤注册接缝（`#[given]`/`#[when]`/`#[then]`）、步骤输入工厂、步骤动词走公开写入口、`world_conn!`/`world_write!`、world 快照七分组与断言强度判据都不随运行器改变；feature 保持 Gherkin 语法与中文业务语言。迁移只改属性语法与占位符形态（`{string}` → `{<参数名>:string}` 等），函数体与断言不复制。
6. **数据表、异步与平台条件跳过各留单点**：数据表步骤经 rstest-bdd 的 `#[datatable]` 参数发放原始行；需要 `await` 的少数步骤（行情抓取桩）统一经既有测试接缝 `test_support::block_on`（tauri 全局运行时）驱动，不新增第二套运行时接线；`@non-root-only` 由「启动器按 tag 过滤」改为场景内守卫步骤调用 `skip!`（保留标签与 `@allow_skipped`），语义与 issue #793 的既有决定一致。
7. **门禁取代「严格编译期步骤校验」**：rstest-bdd 0.6 的 compile-time-validation 只挂在 `#[scenario]` 属性路径，`scenarios!` 生成路径不调用校验；全量改用 `#[scenario]` 意味着 N 个手工绑定且新增场景静默漏跑，代价不可接受。故以静态覆盖守门（feature 步骤行 ↔ 目标注册面全等 + 每个 feature 至少被一个目标绑定，`scripts/check-e2e-step-coverage.ts`）兜底，另在 e2e 入口加**场景数全等守门**（`scenarios!` 生成的场景测试数必须等于 feature 的 `Scenario:` 行数）。

## 否决

- **继续用 cucumber-rs 并调参**：协作式调度下没有让出点，调 `-c` / 并发上限 / worker 数都不产生 CPU 并行（探针实测），且失败置红缺陷需要修自建 harness 才能补上。
- **依赖 `cargo test` 的进程内线程并行**：世界构造（内存库 + 迁移）的 sys 随线程数平方增长，线程并行是负收益；必须配进程级调度。
- **453 个场景全部改 `#[scenario]` 手工绑定**：拿编译期校验换零维护绑定，且新增场景会静默漏跑。
- **自建 `list`/`exact` 协议把 cucumber 塞进 nextest**：上游 cucumber-rs 明确要求自实现协议，工作量与收益不成比例。
- **换其它 BDD 框架**：Rust 侧唯一 Gherkin 运行器即 cucumber-rs；rstest-bdd 是唯一能继承标准测试运行器并行的 Gherkin 方案；Gauge 的 Rust 插件早已停更（spec #1494 已完成选型，本轮不重开）。

## 证据（迁移与收口实测）

收口票 #1508 的观测面 = e2e 段墙钟与 CPU/墙钟比（对照基线 = 旧 cucumber）。

- **迁移前（cucumber，本机 12 核）**：`32.08 real / 27.33 user / 1.38 sys`，CPU/墙钟 ≈ **0.9**（spec #1494 探针）。
- **收口后（本机 12 核）**：`cargo nextest run --workspace --test e2e_rstest` 共 462 个测试（453 场景 + 9 条运行时注册表断言），nextest Summary 34.7s，整命令 `35.28 real / 386.22 user / 9.40 sys`，CPU/墙钟 ≈ **11.2**——并行真实生效。
- **收口前 CI（main run 35437019880，ubuntu-latest 4 vCPU）**：全量 nextest 2214 tests / Summary 209.9s；旧 cucumber 段 451 场景 / 110s。
- **场景数口径**：场景测试数 = feature `Scenario:` 行数 = **453**（本机跑全 453）；CI 容器以 root 运行、`0o555` 目录不可写，2 个 `@non-root-only` 场景经场景内守卫 `skip!` 跳过（451 通过 + 2 跳过）。
- **已知成本（后续优化项）**：rstest-bdd 步骤注册表（当前 725 条模式）在每个测试进程首次匹配时构建，空注册断言测试与场景测试同价（本机 ≈ 0.5s/进程），是 per-test 固定开销的主要来源。

## 后果

- e2e 的并行/过滤/超时/重试/置红全部回归标准测试运行器；场景失败一定让测试进程非零退出，CI 不再吞红灯。
- 失败文本含 feature 路径与原始中文场景名（补偿场景名被 ASCII 清洗后的筛选体验下降）。
- CI 侧 e2e 成为独立一条 nextest 命令并单独计时（build.yml backend job），本地与 CI 口径一致、不出现「本地绿 CI 红」。
- 依赖收窄：根包 dev-deps 移除 cucumber、加入 rstest/rstest-bdd 系。

## 修订记录

- 暂无。
