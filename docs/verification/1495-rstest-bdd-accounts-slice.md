# rstest-bdd 账户域垂直切片（issue #1495 / 父 spec #1494）

> 本票在**不动旧 cucumber e2e 目标**的前提下打通 rstest-bdd 运行通道：新增测试
> 目标把 accounts.feature 的每个 Scenario 绑成独立 `#[test]`，测试世界以 fixture
> 单点提供，账户域消费的步骤函数改为**双注册**（同一函数同时挂两个属性宏，函数体
> 与断言唯一）。本文件记录负向验收证据、同机实测数字、与 spec 决策的一处冲突，
> 以及下游票的分工边界。

## 交付物

- `tests/e2e_rstest.rs`（新测试目标）：`scenarios!` 自动绑定
  accounts.feature（12 个 Scenario → 12 个独立测试），`world` fixture 单点提供测试
  世界；共享支撑模块（world / common / step_inputs / step_verbs）按整文件并入，
  消费者尚是已迁移的步骤域子集，故迁移期挂 `allow(dead_code)`——逐域并入完成后
  本目标即全量目标，该 allow 随之删除、`-D warnings` 重新覆盖这些模块。
- `tests/e2e/world.rs`：`LedgerWorld::new` 放开 `pub(crate)`（旧目标经 cucumber
  `#[world(init = Self::new)]` 消费，新目标经 fixture 消费，同一构造点）。
- `tests/e2e/accounts_steps.rs`：账户域 15 个步骤全部双注册。
- `tests/e2e/transactions_write_steps.rs`：账户域消费的 5 个步骤双注册（「存在账户」
  前置、创建交易、创建转账、创建退款、「应返回错误」断言）；该文件其余步骤归
  issue #1497。
- `Cargo.toml`：dev-dependencies 增 rstest / rstest-bdd / rstest-bdd-macros；旧
  cucumber 依赖与旧目标保留（收口票删除，不做双轨长跑）。

双注册只改属性形态与占位符写法（`{string}` → `{<参数名>:string}`、`{int}` →
`{<参数名>:i64}`、`{float}` → `{<参数名>:f64}`），步骤函数体、断言、步骤动词、
快照分组、公开写入口纪律零改动（CONTEXT-testing「行为等价判据」）。

## 验收证据

1. **场景数 = 测试数**：`grep -c '^  Scenario:' …/accounts.feature` = 12；
   `cargo test --test e2e_rstest` → `12 passed; 0 failed`，测试用时约 0.8s（单进程
   串行；进程级并行归 #1496）。
2. **双注册不伤旧目标**：`cargo test --test e2e` → 40 features / 453 scenarios /
   3139 steps 全绿，场景数与迁移前同口径（453）不减。账户域 12 个场景在新目标
   全绿。
3. **测试世界单点 + 场景间零状态交叉**：每场景一次 `LedgerWorld::new()`（内含接缝
   接线与黑洞账户种子注册，各自独立内存库）；`RUST_TEST_THREADS=12 cargo test
   --test e2e_rstest` 同进程并行 12 场景仍全绿；单场景过滤
   `cargo test --test e2e_rstest accounts___5` → `1 passed; 11 filtered out`。
4. **负向证据（删除即变红）**：
   - 删除 `scenarios!(…)` 绑定 → `running 0 tests`（场景绑定消失的可见形态；此形态
     退出码仍为 0，把它变成红灯的是 issue #1496 的测试执行覆盖/计数守门——
     本票记录该缺口，不在此处另建第二套门禁）。
   - 删除 `world` fixture → 编译失败：`error[E0433]: cannot find module or crate
     'world' in this scope`（场景绑定按 fixture 名解析测试世界）。
   - 附：故意错配一条 rstest-bdd 步骤模式 → 运行时红，失败文本含 feature 路径与
     原始中文场景名，补偿测试名被 ASCII 清洗后的筛选体验：
     `Step not found at index 1: Then 账户列表应包含 1 条记录 (feature:
     tests/e2e/features/accounts.feature, scenario: 创建账户并查看余额)`。
5. **门禁**：`./scripts/check.sh` 全绿（含 `cargo fmt --all -- --check`、
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、结构/文档/
   i18n/测试支撑各守门与 `bun scripts/test-exec.ts check`）；`./scripts/check-docs.sh`
   绿。覆盖守门目标 53 个 = 并发入口 33（集成测试 12，含本票新目标）⊎ 非并发入口
   20，新目标自动落入并发入口，非并发入口名单不变。

### 双注册接线核对清单（`rg` 枚举全部调用点）

「所有步骤都必须双注册」是成对调用型约束，故逐一核对而非抽样：feature 侧 54 条
步骤行（12 个场景）经模式对齐核对，20 条 rstest-bdd 注册全部命中、**未覆盖 0 条**、
无歧义多命中。注册与消费对应如下（15 + 5）：

| 注册出处 | 条数 | 覆盖的 feature 步骤（去引号形态） |
| --- | --- | --- |
| `tests/e2e/accounts_steps.rs` | 15 | 存在账户…初始余额（Given）、存在汇率…兑本位币（Given）、创建账户…初始余额（When）、删除账户（When）、修改账户…名称为（When）、尝试修改账户…币种为（When）、调整账户…余额至…日期（When）、删除上一笔交易（When）、注入余额调整交易写入失败触发器（When）、账户列表应包含 N 条记录（Then）、账户列表应包含/不应包含 X（Then）、账户列表应包含/不应包含黑洞账户 X（Then）、X 账户余额应为 N（Then） |
| `tests/e2e/transactions_write_steps.rs` | 5 | 存在账户…币种（Given）、创建交易 类型…金额…到账户…日期（When）、创建转账 金额…从…到…日期（When）、关联上一笔交易创建退款 金额…日期（When）、应返回错误 X（Then） |

核对口径：把 20 条 rstest-bdd 模式按 `{<名>:string}` → 带引号字符串、
`{<名>:i64}` → 整数、`{<名>:f64}` → 小数转成匹配式，再逐条匹配 feature 步骤行
（`And`/`But` 继承前一关键字，与 Gherkin 语义一致）。该口径只证模式覆盖与唯一性，
运行期匹配由「12 场景全绿」背书（模式错配即 `Step not found`，见负向证据 4）。

## 与 spec 决策的冲突（显式报告，未自行改实现或决策）

spec #1494 的实现决策要求「开启严格编译期校验（missing/ambiguous 步骤在编译期
报错），作为机械迁移的兜底」。实测在本形态下**不成立**：

- rstest-bdd 0.6.0 的 `compile-time-validation` / `strict-compile-time-validation`
  只挂在 `#[scenario]` 属性路径——`validate_steps_exist` 的唯一非测试调用点是
  `macros/scenario/mod.rs`；`scenarios!` 生成路径（`codegen::scenario::generate_scenario_code`）
  不调用校验。
- 复现：dev-dependency 打开 `strict-compile-time-validation` 后，故意把一条步骤模式
  改错，编译仍通过，场景运行时才以 `Step not found …` 变红（`cargo tree -e
  features -i rstest-bdd-macros` 确认 feature 确已开启）。

故本票**不打开**该 feature（避免「看着有门禁、实际不设防」）；宏展开前的步骤库
缺失/歧义在 `scenarios!` 形态下仍是运行期红灯。机械迁移的编译期兜底需另择手段
（静态步骤库守门 / 改用 `#[scenario]` 单点 / 上游修复），属 issue #1497 或 spec
收口决策，本票只报告冲突。

## 下游分工与未验证项

- 进程级 per-test 调度（cargo-nextest）、两个 e2e 目标都入调度面与漏跑守门归
  issue #1496；本票只交付「场景 = 普通测试」这一接缝。
- 交易写入主干整文件双注册与其余域逐域迁移归 issue #1497 及后续票。
- 进程内 libtest 线程并行对世界构造（内存库 + 迁移）的负收益数字（1/4/12 线程
  = 0.41/0.58/0.88s、sys 随线程数上升）未在本票复测，以 spec #1494 正文的实测
  记录为准——spike 分支与 worktree 已随本票交付清理（其独有产出即该组数字，
  已归档在 spec 正文）；本票的线程并行运行只作为「场景间零状态交叉」的证据，
  不作为并行方案。
