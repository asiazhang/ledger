# 保单域迁入新目标（issue #1501 / 父 spec #1494）

> 本票把保单域三份 feature——`policies.feature`（11 Scenario）/`policy_agreement.feature`
> （10）/`policy_stats.feature`（8），共 29 个 Scenario——绑进 rstest-bdd 新目标
> （`tests/e2e_rstest.rs`），并把其消费的步骤函数改为**双注册**（同一函数同时挂
> cucumber 与 rstest-bdd 属性宏）。改写只涉及属性语法与占位符形态
> （`{string}` → `{<参数名>:string}`、`{int}` → 有符号/无符号整数 hint），函数体、
> 断言、步骤动词与快照分组纪律零改动；旧 cucumber 目标行为零变化。本文件记录
> 场景数口径、双注册接线核对与负向证据。

## 交付物

- `tests/e2e_rstest.rs`：新增 3 条 `scenarios!` 绑定（policies 11 / policy_agreement 10 /
  policy_stats 8），并按 `#[path]` 并入 `policy_agreement_steps.rs`、
  `policy_stats_steps.rs` 与共享步骤父模块 `scheduled_steps.rs`；新增运行时注册表
  断言 `policy_steps_are_registered_in_rstest_bdd`（三文件 20 / 11 / 7 条注册与
  string / i64 / usize / 无占位符命中，删任一新注册即红）。
- `tests/e2e/policies_steps.rs`（20）、`policy_agreement_steps.rs`（11）、
  `policy_stats_steps.rs`（7）：整文件双注册，共 38 条。
- 按需双注册其消费的共享步骤（**本轮新增 6 条**）：`insurers_steps.rs` 1 条（软删保司；
  `存在保司` 已由 #1499 双注册）、
  `scheduled_steps/occurrence.rs` 2 条（执行该计划第一期、该期次交易类型应为 … 金额应为 …）、
  `scheduled_steps/spend.rs` 3 条（执行该计划前 N 期、取消该订阅计划、暂停该订阅计划）。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

双注册只改属性形态与占位符写法，参数名与函数形参对齐，引号剥离与数值解析语义与
cucumber 一致（CONTEXT-testing「行为等价判据」）。

## 验收证据

1. **AC1 场景数 = feature 内 Scenario 数（新目标逐 feature 全等）**：
   `grep -c '^  Scenario:'` → policies 11 · policy_agreement 10 · policy_stats 8；
   `cargo nextest run --test e2e_rstest` 全量 81 tests / 81 passed / 0 skipped，
   其中本票三份 feature 生成的测试逐 feature 计数与之一致（`cargo nextest list`
   过滤 `policies_feature_scenarios` / `policy_agreement_feature_scenarios` /
   `policy_stats_feature_scenarios` 各 11 / 10 / 8），另 1 条为本票运行时注册表断言。
2. **AC2 旧 e2e 目标全绿、场景数不减**：`cargo test --workspace --test e2e` →
   40 features / **453 scenarios（453 passed）** / 3139 steps（3139 passed），
   与迁移前同口径。双注册对新目标之外零影响（cucumber 注册面与函数体一字未动）。
3. **静态覆盖守门**：`bun scripts/check-e2e-step-coverage.ts` → 目标 2 个 · 绑定
   feature 40 个 · 步骤行 3667 条 · 注册 875 条 · **未覆盖 0 · 歧义 0 · 无绑定 0**；
   新目标：绑定 feature 8 个 · 注册 151 条 · 步骤 528 条。即被绑 feature 的每条步骤行
   在新目标注册面恰好一次命中，删任一注册即红。
4. **门禁**：`./scripts/check.sh` 全绿（EXIT=0，含 `cargo fmt --all --check`、
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、
   结构 / 文档 / i18n / 测试支撑 / 测试执行覆盖 / e2e 步骤库覆盖各守门）。
5. **全量测试**：`./scripts/test.sh` 退出码 0——并发入口 32/32 个二进制
   （1788 passed / 0 failed / 4 ignored），e2e 新目标 81/81（含本票 29 个场景），
   旧目标 40 features / 453 scenarios / 3139 steps 全绿，doc-test 各包通过。

### 双注册接线核对清单（`rg` 枚举全部调用点）

「本票纳入的步骤文件所有步骤都必须双注册」是成对调用型约束，故逐文件核对而非抽样
（`rg -c '^#\[rstest_bdd_macros::(given|when|then)\('` ⇔ `rg -c '^#\[(given|when|then)\('`，
同一函数上下相邻两条属性、同体）：

| 步骤文件 | rstest ⇔ cucumber | 说明 |
| --- | --- | --- |
| `policies_steps.rs` | 20 ⇔ 20 | 整文件 |
| `policy_agreement_steps.rs` | 11 ⇔ 11 | 整文件 |
| `policy_stats_steps.rs` | 7 ⇔ 7 | 整文件 |
| `insurers_steps.rs` | 2 ⇔ 15 | 按需（存在保司 + 软删保司） |
| `scheduled_steps/occurrence.rs` | 2 ⇔ 14 | 按需（执行该计划第一期 + 该期次交易类型应为 … 金额应为 …） |
| `scheduled_steps/spend.rs` | 3 ⇔ 13 | 按需（执行该计划前 N 期 + 取消/暂停该订阅计划） |

其余跨文件共享步骤（`存在账户…`、`创建交易…`、`删除最近交易`、`应返回错误…` 等）由
#1495 / #1497 / #1498 / #1499 已整文件双注册，本轮直接复用；无其它跨文件依赖
（由静态覆盖守门「未覆盖 0」背书）。

## 负向证据（删除即变红，实测）

1. **删注册 → 红**：临时删去 `policy_stats_steps.rs` 的
   `#[rstest_bdd_macros::then("保单 {number:string} 到期态应为 {expected:string}")]`
   （只留 cucumber）→
   - 静态覆盖守门红：3 处「未覆盖」（`policy_stats.feature:96/97/98`）；
   - 新目标已绑场景运行时红：
     `scenarios::policy_stats_feature_scenarios::policy_stats___7` →
     `Step not found at index 6: Then 保单 "P2026-309" 到期态应为 "已到期"`
     （feature: `policy_stats.feature`, scenario: 到期态推导——…）；
   - 常驻运行时断言红：`policy_steps_are_registered_in_rstest_bdd` 注册条数
     `left: 6, right: 7`（`policy_stats_steps.rs`），使该负向证据可复现、非一次性手工实验。
   恢复该注册后门禁与场景复绿。
2. **删绑定 → 新目标场景消失**：临时删去 `scenarios!("tests/e2e/features/policies.feature")`
   → 新目标 `cargo nextest list` 由 81 条降到 70 条（policies 的 11 个场景测试消失）。
   **证据边界**：迁移双轨期旧 cucumber 目标仍绑定该 feature，故静态守门的「无绑定」
   分支**不**触发——该分支在设计上守的是「feature 至少被一个 e2e 目标绑定」，收口
   （#1508 删除旧目标）后同一删绑定即触发「无绑定 1」红。本票在双轨期以「新目标
   场景数计数」担此负向证据的一半，另一半由覆盖守门的注册面全等承担。

## 证据边界（不夸大）

- 双注册的同一函数承载两份步骤文本（cucumber `{string}` 与 rstest-bdd `{<名>:type}`）：
  静态守门对两族各自与 feature 步骤行比对，故**跨族文本漂移**（rstest 侧 hint 写成
  合法但语义不同的整数族）不会被守门直接拦住——已由逐 feature 场景运行（消费侧）
  覆盖；改写时以函数形参类型为准。
- `scheduled_steps.rs` 按整模块并入新目标（其子模块被编译器要求成组解析），本票只为
  其被消费的 5 条步骤补注册；其余定时计划步骤的迁移归后续票。
- 迁移期的 `allow(dead_code)`（`tests/e2e_rstest.rs` 顶部）随最后一个域并入删除，
  不属本票范围。
