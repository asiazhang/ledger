# 交易编辑与查询场景迁入新目标（issue #1498 / 父 spec #1494）

> 本票把 `transactions_edit` / `transactions_query` 步骤文件整文件双注册，并把
> `transactions_write` / `transactions_edit` / `transactions_query` 三个 feature
> 绑进 rstest-bdd 新目标；三个 feature 消费的其它共享步骤文件按需双注册其步骤。
> 改写只涉及属性语法与占位符形态，断言、步骤动词与公开写入口纪律零改动。唯一
> 结构性例外是 `migration_steps.rs` 的数据表步骤：cucumber 的 `#[step] &Step` 与
> rstest-bdd 的 `#[datatable] Vec<Vec<String>>` 不可同签名共存，故把写入逻辑抽到
> `run_batch_import` 单点，两侧只留注册适配器；行解析与写入行为逐字保持，由两侧
> 全量测试背书。旧 cucumber 目标行为零变化。本文件记录场景数口径、静态覆盖守门、
> 负向证据、双注册接线核对清单与未迁移边界。

## 交付物

- `tests/e2e/transactions_edit_steps.rs`：15 条步骤整文件双注册（When 10 +
  Then 5），占位符按 `{string}` → `{<参数名>:string}`、`{int}` → `{<参数名>:i64}`
  改写，参数名与函数形参对齐。
- `tests/e2e/transactions_query_steps.rs`：20 条步骤整文件双注册（When 3 +
  Then 17），同上口径。
- 三个 feature 消费的其它共享步骤按需双注册：
  - `dashboard_steps.rs`：`存在标的 … 币种 …` 1 条（交易写入/编辑消费）；
  - `fund_trade_steps.rs`：基金标的、确认单申赎、基金明细/份额/已实现盈亏断言
    7 条（交易写入消费）；
  - `merchants_steps.rs`：存在商户、软删商户、带商户创建交易、商户列表/含软删
    列表断言 6 条（交易查询消费）；
  - `categories_steps.rs`：5 条整文件（交易查询消费）；
  - `migration_steps.rs`：`批量导入交易` 1 条——数据表步骤的 cucumber 入参
    （`#[step] &Step`）与 rstest-bdd 入参（`#[datatable] Vec<Vec<String>>`）不可同
    签名共存，故抽出 `run_batch_import(world, rows)` 单点承载写入逻辑，两侧各留
    注册适配器。
- `tests/e2e_rstest.rs`：新增 7 个步骤模块引用 + 3 个 `scenarios!` 绑定
  （transactions_write / transactions_edit / transactions_query），测试世界仍由同一
  `world` fixture 单点提供。
- 无生产代码、HTTP API 契约、数据模型与迁移改动。

## 验收证据

1. **AC1 两个步骤文件整文件双注册 / AC2 三个 feature 在新目标全绿**：
   `cargo nextest run --test e2e_rstest` → `43 tests run: 43 passed, 0 skipped`
   （0.855s）：
   - accounts.feature 12；
   - transactions_write.feature 11；
   - transactions_edit.feature 7；
   - transactions_query.feature 12；
   - 既有运行时注册表断言 1（#1497）。
   场景数 = feature 内 Scenario 数（11 + 7 + 12 = 30），无静默漏跑或重复绑定。
2. **AC2 静态覆盖守门**：`bun scripts/check-e2e-step-coverage.ts` → 目标 2 个 ·
   绑定 feature 40 个 · 步骤行 3403 条 · 注册 816 条（rstest-bdd 92 / cucumber
   724）· 未覆盖 0 · 歧义 0 · 无绑定 0；其中 `tests/e2e_rstest.rs` 绑定 feature
   4 个、注册 92 条、步骤行 264 条。新目标注册面构成为账户 15 + 交易写入 22 +
   交易编辑 15 + 交易查询 20 + 仪表盘 1 + 基金 7 + 商户 6 + 分类 5 + 迁移 1。
3. **AC3 负向证据（删除即变红）**：
   - 删 `transactions_edit_steps.rs` 的「修改最近交易」rstest-bdd 注册（只留
     cucumber）→ 覆盖守门报 `transactions_edit.feature:7` 未覆盖；运行时
     `transactions_edit__id` 红：`Step not found at index 2: When 修改最近交易 …`。
   - 删 `migration_steps.rs` 的数据表注册壳 → 覆盖守门报 4 处 `批量导入交易`
     未覆盖；transactions_query 4 个消费该步骤的场景红（同 `Step not found`）。
   - 恢复后覆盖守门复绿、e2e_rstest 43/43 复绿。
4. **AC3 旧目标行为零变化**：`cargo test --test e2e` → 40 features / 453
   scenarios (453 passed) / 3139 steps (3139 passed)，与迁移前同口径。
5. **门禁与全量测试**：`./scripts/check.sh` 全绿（含
   `cargo clippy --workspace --all-targets --all-features -- -D warnings`、结构/
   文档/i18n/测试支撑/测试执行覆盖/步骤库覆盖/异步守门）；`./scripts/test.sh`
   全绿：并发入口 32/32 二进制（1788 passed / 0 failed / 4 ignored）+ nextest
   e2e_rstest 43/43 + 旧 e2e 453 scenarios + doc-test 各包通过。

## 双注册接线核对清单

| 文件 | cucumber 注册 | 本票 rstest-bdd 注册 | 口径 |
| --- | --- | --- | --- |
| `transactions_edit_steps.rs` | 15 | 15 | 整文件全量 |
| `transactions_query_steps.rs` | 20 | 20 | 整文件全量 |
| `categories_steps.rs` | 5 | 5 | 整文件（消费集 = 文件全量） |
| `dashboard_steps.rs` | 9 | 1 | 按需（`存在标的 … 币种 …`） |
| `fund_trade_steps.rs` | 12 | 7 | 按需（交易写入消费的子集） |
| `merchants_steps.rs` | 18 | 6 | 按需（交易查询消费的子集） |
| `migration_steps.rs` | 18 | 1 | 按需（`批量导入交易`，双壳共用实现） |

核对口径：`rg -c '^#\[(rstest_bdd_macros::)?(given|when|then)'` 逐文件与上表
相等；新注册占位符 100% 带类型提示，逐条与 cucumber 模式同文本、仅占位符命名/
类型化；`transactions_edit` / `transactions_query` 的每条注册都由 #1510 覆盖守门
对 feature 步骤行逐条全等判定（未覆盖 0 / 歧义 0）。

## 证据边界（不夸大）

- 共享步骤文件按需注册的**未消费步骤**仍只挂 cucumber 注册，由后续票在这些 file
  继续补齐：`fund_trade_steps` 余 5 条归 #1502（fund_trade 场景），
  `merchants_steps` 余 12 条归 #1505（参考数据与检索），`dashboard_steps` 余 8 条
  与 `migration_steps` 余 17 条归 #1504 / #1505 / #1507 等消费票；本票不扩大其
  文件范围。
- 旧 cucumber 目标与 `cucumber` dev-dependency 由 spec #1494 收口票 #1508 删除，
  不做双轨长跑；迁移期 `allow(dead_code)` 亦随最后一个域并入删除。
- `docs/verification/1496-e2e-nextest.md` 中 e2e_rstest 的旧计数（12 + 1）是本票
  之前的快照；nextest 调度面与命令不变，本票只把测试数扩至 43。
