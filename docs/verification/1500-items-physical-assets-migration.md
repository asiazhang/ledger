# 物品与实物资产域迁入新目标（issue #1500 / 父 spec #1494）

> 本票把物品域（items_*）与实物资产域（physical_asset*）共 8 个 feature 绑进
> rstest-bdd 新目标（`tests/e2e_rstest.rs`），并把两域消费的步骤函数改为**双注册**
> （同一函数同时挂 cucumber 与 rstest-bdd 属性宏）。改写只涉及属性语法与占位符形态，
> 函数体、断言、步骤动词与快照分组纪律零改动；旧 cucumber 目标行为零变化。
> 本文件记录场景数口径、双注册接线核对、顺带定位到的既有缺陷根因与证据边界。

## 交付物

- `tests/e2e_rstest.rs`：新增 8 条 `scenarios!` 绑定（items_cost / items_create /
  items_dispose / items_provenance / items_update / physical_assets /
  physical_asset_updates / physical_asset_disposal），并按 `#[path]` 并入 8 个步骤文件、
  物品共享辅助 `items_common.rs` 与共享步骤父模块 `scheduled_steps.rs`。
- `tests/e2e/items_cost_steps.rs`（11）、`items_create_steps.rs`（13）、
  `items_dispose_steps.rs`（12）、`items_provenance_steps.rs`（10）、
  `items_update_steps.rs`（10）：整文件双注册，共 56 条。
- `tests/e2e/physical_assets_steps.rs`（18，含 `创建实物资产` 的 Given/When 双注册）、
  `physical_asset_updates_steps.rs`（6）、`physical_asset_disposal_steps.rs`（7）：
  整文件双注册，共 31 条。
- `tests/e2e/scheduled_steps/occurrence.rs`：**按需**双注册其被消费的 1 条共享步骤
  `存在汇率 X 兑 Y 为 R`（跨域汇率夹具，items_* / physical_asset* 依赖它）。该文件
  其余 13 条定时计划步骤不属本票，归 ticket #1506（其父模块已随本票并入新目标，
  #1506 只需补注册与绑定 scheduled.feature）。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

双注册只改属性形态与占位符写法（`{string}` → `{<参数名>:string}`、`{int}` →
有符号/无符号整数 hint、`{float}` → `{<参数名>:f64}`），参数名与函数形参对齐，引号
剥离与数值解析语义与 cucumber 一致（CONTEXT-testing「行为等价判据」）。

## 验收证据

1. **AC1 场景数 = feature 内 Scenario 数（新目标逐 feature 全等）**：
   `cargo nextest list --test e2e_rstest` 按 feature 计数——
   items_cost 11 · items_create 6 · items_dispose 12 · items_provenance 12 ·
   items_update 6 · physical_assets 13 · physical_asset_updates 12 ·
   physical_asset_disposal 9（与 `grep -c '^  Scenario:' …/<feature>` 逐一相等）；
   新目标测试总数 94 = 93 个场景（含 #1495 账户域 12）+ 1 条 #1497 注册表断言。
2. **AC2 旧 e2e 目标全绿、场景数不减**：`cargo test --test e2e` →
   40 features / **453 scenarios（453 passed）** / 3139 steps（3139 passed），
   与迁移前同口径（场景数、步数均不减）。双注册对新目标之外零影响
   （cucumber 注册面与函数体一字未动）。
3. **AC3 issue #1489 的既有失败不并入本票**：见下「根因发现」——本票**未修**该缺陷，
   仅顺带定位根因并说明另一个同根因实例（ticket 正文的「如顺带消除需说明根因」）。
4. **静态覆盖守门**：`bun scripts/check-e2e-step-coverage.ts` → 目标 2 个 · 绑定
   feature 40 个 · 步骤行 3601 条 · 注册 849 条（rstest-bdd 125 / cucumber 724）·
   未覆盖 0 · 歧义 0 · 无绑定 0；其中新目标绑定 feature 9 个 · 注册 125 · 步骤 462。
   即被绑 8 个 feature 的每条步骤行在新目标注册面**恰好一次**命中，删任一注册即红。
5. **门禁**：`./scripts/check.sh` 全绿（含 `cargo fmt --all -- --check`、
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、结构 / 文档 /
   i18n / 测试支撑 / 测试执行覆盖 / e2e 步骤库覆盖各守门）。

### 双注册接线核对清单（`rg` 枚举全部调用点）

「本票纳入的步骤文件所有步骤都必须双注册」是成对调用型约束，故逐文件核对而非抽样
（`rg -c 'rstest_bdd_macros::(given|when|then)\('` ⇔ `rg -c '#\[(given|when|then)\('`，
同一函数上下相邻两条属性、同体）：

| 步骤文件 | 条数 | rstest ⇔ cucumber |
| --- | --- | --- |
| `items_cost_steps.rs` | 11 | 11 ⇔ 11 |
| `items_create_steps.rs` | 13 | 13 ⇔ 13 |
| `items_dispose_steps.rs` | 12 | 12 ⇔ 12 |
| `items_provenance_steps.rs` | 10 | 10 ⇔ 10 |
| `items_update_steps.rs` | 10 | 10 ⇔ 10 |
| `physical_assets_steps.rs` | 18 | 18 ⇔ 18 |
| `physical_asset_updates_steps.rs` | 6 | 6 ⇔ 6 |
| `physical_asset_disposal_steps.rs` | 7 | 7 ⇔ 7 |

两域消费的跨文件共享步骤来源：`transactions_write_steps.rs`（`存在账户…`、
`创建交易…`、`应返回错误…`，#1497 已整文件双注册）、`scheduled_steps/occurrence.rs`
（`存在汇率 X 兑 Y 为 R`，本票按需双注册）；无其它跨文件依赖（由静态覆盖守门
「未覆盖 0」背书）。

## 根因发现：issue #1489 与 `physical_assets.feature:73` 同一根因（未修）

跑新目标时两条断言间歇性红，定位为同一根因的**既有缺陷**（本机 `main` 旧目标同样
复现，非本票引入）：

- `physical_asset_updates.feature:20`（正对应 issue #1489）：更新估值后详情/列表
  读到旧值 5000000（应为 6000000）。
- `physical_assets.feature:73`（同根因的另一个实例）：多资产列表按「第 1 件」定位时
  偶发取错行（`第 1 件资产当前估值折本位币应为 72000` 拿到 30000）。

**根因**：读路径用「时间列 + `id`」做排序/取最新，而 `id` 是**同一毫秒内非单调**的
UUID v7，时间列精度又不足以区分相邻写入——

- `crates/physical-asset/src/crud.rs`：资产列表 `ORDER BY a.created_at, a.id`；
  当前估值取最新 `ORDER BY v2.valuation_date DESC, v2.id DESC LIMIT 1`。
- `crates/infra/src/ids.rs`：`now_iso` = `%Y-%m-%dT%H:%M:%SZ`（**秒**精度）、
  `new_uuid` = `Uuid::new_v7(Timestamp::now(NoContext))`——UUID v7 的时间戳为**毫秒**
  且同毫秒内低 42 位随机，故同一毫秒生成的两个 id 排序随机；`valuation_date`（**日**
  精度）同理。

**证据**：失败运行中打印列表行——`海外车位 id=01a0b74e-03ad-7739…` 与
`国画 id=01a0b74e-03ad-72a6…` 落在**同一毫秒** `03ad`，随机低位使 `国画` 排到
`海外车位` 之前。`main` 工作区（`d2867e88`，旧 cucumber 目标）对本场景连跑 8 次
**1 次失败**，证明与调度/本票无关。

**处置**：按 ticket 正文「#1489 的既有失败不并入本票」**不修**（修法则落在
id 单调性或排序口径的设计面，不属本票迁移范围）。本票只登记该根因与同根因实例，
供 #1489 收口。

## 证据边界（不夸大）

- 新目标本次全量运行实测 93/94 通过（仅 #1489 场景红）；另一次 92/94（叠加
  `physical_assets.feature:73`）。两处失败均为上节同根因的既有缺陷，删除本票任一双
  注册会多出「步骤未找到」类红，与上述根因无关。
- rstest-bdd 0.6.0 的 `scenarios!` 形态不做严格编译期步骤校验（#1495 已报告、#1494
  裁决改为静态覆盖守门 #1510）；本票的逐条语义全等由该守门与逐 feature 场景计数背书。
- `scheduled_steps.rs` 按整模块并入新目标（其子模块被编译器要求成组解析），本票只
  为其被消费的 1 条步骤补注册；其余定时计划步骤的迁移归 #1506。
- 迁移期的 `allow(dead_code)`（`tests/e2e_rstest.rs` 顶部）随最后一个域并入删除，
  不属本票范围。
