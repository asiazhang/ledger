# 投资与行情域场景迁入新目标（issue #1502 / 父 spec #1494）

> 本票把 `instruments.feature` 的 34 个 Scenario 与 `manual_quote.feature` 的 5 个
> Scenario 绑进 rstest-bdd 新目标（`tests/e2e_rstest.rs`），并把两个 feature 消费的
> 步骤文件**整文件双注册**：instruments 32 条、manual_quote 6 条、investment_trend
> 9 条、investment_migration 4 条、fund_trade 12 条（#1498 先行 7 条，本票补齐 5 条），
> 另按需补 dashboard 3 条与 migration 2 条共享步骤。旧 cucumber 目标行为零变化。
> 本文件记录验收证据、双注册接线核对清单、异步步骤的唯一接线点与负向证据。

## 交付物

- `tests/e2e_rstest.rs`：新增 `scenarios!("tests/e2e/features/instruments.feature")`
  与 `scenarios!("tests/e2e/features/manual_quote.feature")` 绑定（34 + 5 个 Scenario
  → 独立测试）；并入所需步骤模块；新增运行时注册表断言
  `investment_domain_steps_are_registered_in_rstest_bdd`（五个步骤文件的注册条数与
  占位符语义，含异步三态、数据表步骤与 `f64` 小数）。
- `tests/e2e/instruments_steps.rs`：整文件 32 条双注册。股票通道三条行情抓取桩步骤
  需要 `await`，按「共享 async 实现 + 两侧注册适配器」拆为
  `add_instrument_with_stub_*_impl`（共享体）/ `*_cucumber`（旧目标，共享 runner 的
  tokio 运行时内直接 `await`）/ `*_rstest`（新目标，经唯一接缝 `block_on`）。
- `tests/e2e/manual_quote_steps.rs`（6 条）、`tests/e2e/investment_trend_steps.rs`
  （9 条）、`tests/e2e/investment_migration_steps.rs`（4 条）、
  `tests/e2e/fund_trade_steps.rs`（补齐 5 条至 12 条）整文件双注册；占位符按参数名与
  类型改写（`{string}` → `{<名>:string}`、`{int}` → `{<名>:i64|usize}`、`{float}` →
  `{<名>:f64}`），函数体与断言不变。
- `tests/e2e/dashboard_steps.rs` 按需 3 条、`tests/e2e/migration_steps.rs` 按需 2 条：
  两个 feature 消费的跨域共享步骤（净资产总览与余额读回）。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

## 验收证据

1. **AC1 场景数 = feature 场景数且全绿**：`grep -c '^  Scenario:'` →
   instruments 34 / manual_quote 5；`cargo test --test e2e_rstest` → `91 passed;
   0 failed`（12 账户域 + 11 交易写入 + 7 交易编辑 + 12 交易查询 + 7 交易×保单 +
   34 标的 + 5 手动报价 + 3 条注册表断言；新增场景 39 个全部绿）。
2. **AC2 旧 e2e 目标全绿**：`cargo test --test e2e` → 40 features /
   453 scenarios (453 passed) / 3139 steps (3139 passed)，场景数与步数均与迁移前
   同口径（双注册不改旧注册面）。
3. **AC3 异步步骤接线为单点**：三条行情抓取桩步骤的新目标适配器只经一个函数
   `instruments_steps::block_on` 转发到既有测试接缝
   `tauri_app_lib::test_support::block_on`（tauri 全局运行时）；步骤文件内零
   `tauri::async_runtime::block_on` / `tokio::task::block_in_place` 直呼，未新增
   第二套运行时接线。实测：本票迁移对象内**不存在** `block_in_place` 形态——
   仓内唯一一处是 `tests/e2e/sync_steps.rs`（多端同步域，归 #1507），故本票的
   「退役 block_in_place」落点是「不为行情抓取桩引入第二套运行时」，显式报告。
4. **负向证据（删除即变红）**：
   - **接线单点删除（全敏感面）**：接线单点 = `instruments_steps::block_on`（转发到既有
     接缝 `tauri_app_lib::test_support::block_on` 的唯一函数）。把它的函数体换成不驱动
     future 的形态（保留三条适配器注册）→ 新目标 **5 红**：沪市 / 深市 / 港股 / 美股
     通道四个行情命中场景，加「查询未命中与临时不可达均显式报错不建档」场景（三条异步
     步骤的全部 5 个消费场景），红在步骤本身，如
     `Step failed at index 0: When 按代码添加投资标的 市场 "sh" 代码 "600999"
     行情查无此码`。恢复接线点后复绿。
   - **单条适配器调用删除（可观察断言形态）**：把其中一条适配器的 `block_on(...)` 换成
     不驱动 future 的占位（保留注册）→ 4 红在 Then 断言 `标的 600519（stock）应存在`
     ——同一接线的删除以「字典行未落库」这一用户可观察形态置红，而非仅 panic。
   - 删除异步步骤 `按代码添加投资标的 市场 … 行情查无此码` 的 rstest 注册（只留
     cucumber 适配）→ 2 红：注册表断言 `left: 31, right: 32`；已绑场景红在
     `Step not found at index 0: When 按代码添加投资标的 市场 "sh" 代码 "600999"
     行情查无此码 (feature: tests/e2e/features/instruments.feature, scenario:
     查询未命中与临时不可达均显式报错不建档（建档归自定义标的通道，issue #826）)`。
     恢复注册后复绿。
5. **静态覆盖守门（#1510 口径）**：`bun scripts/check-e2e-step-coverage.ts` →
   目标 2 个 · 绑定 feature 40 个 · 步骤行 3682 条 · 注册 893 条 · 未覆盖 0 ·
   歧义 0 · 无绑定 0；新目标 `tests/e2e_rstest.rs`：绑定 feature 7 · 注册 169 ·
   步骤 543（新增的 instruments + manual_quote 216 条步骤行全部命中）。
6. **门禁与全量测试**：`./scripts/check.sh` 全绿（含
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、结构/文档/
   i18n/测试支撑各守门与 e2e 步骤库覆盖守门）；`./scripts/test.sh` 退出码 0：
   并发入口 32/32 二进制（1788 passed / 0 failed / 4 ignored）+ e2e 入口（新目标
   nextest `91 tests run: 91 passed, 0 skipped`（3.31s）、旧目标 453 scenarios）+
   doc-test 各包通过。

## 双注册接线核对清单（`rg` 枚举全部调用点）

「被 feature 消费的步骤都必须双注册」是成对调用型约束，故逐一核对而非抽样
（口径：`rg -c '^#\[(given|when|then)\('` ↔ `rg -c 'rstest_bdd_macros::(given|when|then)'`，
同一函数上下相邻两条属性，函数体与断言唯一）：

| 注册出处 | cucumber ↔ rstest | 覆盖面 |
| --- | --- | --- |
| `instruments_steps.rs` | 32 ↔ 32 | 存在标的/类型/市场（Given）、手动创建、删除、列出、搜索（含类型过滤）、按 id 取（含未知 id）、按代码添加基金三态、股票通道行情桩三态（异步）、字典行/条数/现价/净值日期/错误断言 |
| `manual_quote_steps.rs` | 6 ↔ 6 | 录价（When）、现价缓存、价格历史条数、周点价格与来源、持仓视图市值 / 未实现盈亏（Then） |
| `investment_trend_steps.rs` | 9 ↔ 9 | 价格历史与汇率历史夹具、买入 / 卖出标的、查询组合走势与单标的走势、周点计数 / 指定周市值断言 |
| `investment_migration_steps.rs` | 4 ↔ 4 | 幂等创建标的、批量导入投资交易（数据表）、导入结果行数、标的持仓读回 |
| `fund_trade_steps.rs` | 12 ↔ 12 | 存在基金标的、按确认单申购 / 赎回（含指定日期与出资账户形态）、买卖明细断言、持仓份额、已实现盈亏、买入双账户匹配（本票补齐后 5 条） |
| `dashboard_steps.rs` | 按需 3 | 查询净资产总览、净资产应为、持仓市值合计应为（manual_quote 场景消费） |
| `migration_steps.rs` | 按需 2 | 查询全部账户余额、账户 X 余额应为（投资迁移链路场景消费） |

## 三处形态差异（显式报告）

1. **异步步骤无法共用同一签名**：cucumber 侧步骤体是 `async fn`（由共享 runner 的
   tokio 运行时 poll），rstest-bdd 步骤体是同步函数。故三条行情抓取桩步骤保留
   **一个共享 async 实现 + 两个薄适配**，两侧写入与断言语义仍唯一；新目标侧统一经
   `block_on`（既有 `test_support::block_on`，tauri 全局运行时），与 spec #1494
   「需要异步的少数步骤有唯一接线点」一致。
2. **数据表步骤同样无法共用同一签名**：`批量导入投资交易` 在 cucumber 侧经
   `#[step] &gherkin::Step` 注入、rstest-bdd 侧只认 `Vec<Vec<String>>`（`#[datatable]`），
   故与 `migration_steps::批量导入交易` 同款「一个共用实现 `run_batch_import_trades`
   + 两个薄适配」。
3. **`instruments_steps.rs` 的 `Result` 别名**：`use ledger_infra::error::Result as
   AppResult` 为 #1499 遗留（rstest-bdd 生成的 wrapper 使用未限定的
   `Result<StepExecution, StepError>`），本票不改动其形态。

## 与相邻票的边界

- 本票只扩展新目标的绑定面与步骤注册面，不删旧 cucumber 目标与 `cucumber`
  dev-dependency（收口票 #1508）。
- `financial_freedom.feature` 归 #1504（报表与仪表盘域票，其依赖本票）；`books` /
  `migration` / `sync` 等引导类场景归 #1507；`transactions_convert` /
  `transactions_funding` / `transactions_source` 归 #1503——本票把 fund_trade 步骤
  文件补齐到整文件双注册，正是为 #1503 备好投资侧消费面。
- 迁移期的 `allow(dead_code)`（`tests/e2e_rstest.rs` 顶部）随最后一个域并入删除，
  不属本票范围。
