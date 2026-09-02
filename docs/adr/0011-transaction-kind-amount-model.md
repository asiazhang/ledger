# ADR 0011: 交易 kind→度量归属与 raw/native 金额分离收口为 transaction 领域模块

- 状态：已接受
- 日期：2026-08-25
- 作者：Ledger 项目

## 背景

「交易如何落库」与「金额如何折算」在 Ledger 里没有单一权威，导致三类问题：

1. **`INSERT INTO transactions` 列清单多处重复。** 创建/修改、买入/卖出、定时引擎、导入各自手写插入列清单，加列要同步多处分号对齐，极易错位。
2. **`CASE WHEN` 金额口径散落多处。** 账户余额、报表、预算各自维护一套 kind→符号的 SQL 表达式，改一处口径要改多处，测试还容易复制生产 SQL 而"测个寂寞"。
3. **refund（退款）的符号在三处互相冲突。** 账户余额按 `+`（钱退回账户）、支出净额/预算按 `−`（冲减支出）、月度汇总又单独成列。根因是没有"kind → 度量"的统一定义——单一符号无法同时满足这些度量。定时引擎还曾绕过节算把 `amount_native_cents` 直接写成原始金额（非本位币即静默存错）。

这不是新功能，而是一次**行为保持的重构**（spec #52，子任务 #54–#61 全部完成）：把分散的写入与口径收进一个深模块，消灭重复与漂移，并顺带修复定时引擎的折算 bug。

## 决策

1. **`transaction` 领域模块为唯一真源，含两个接缝：**
   - **Writer 接缝**（`src-tauri/src/transaction/writer.rs`）：列映射 + 字段归一化 + 本位币折算 + INSERT/UPDATE。`normalize`（通用 kind：金额>0、transfer 必须有 `to_account_id`、refund 继承原支出账户/币种/分类）+ `insert_row`（模块内生成 id 与审计字段）+ `update_row`（保留 created_at 与幂等身份，version 递增）。所有写入路径（创建/修改、买入/卖出行、定时引擎、批量导入）都经它落库；幂等/去重、事务边界、buy/sell 持仓副作用留在命令层编排。
   - **Amount 接缝**（`src-tauri/src/transaction/amount.rs`）：`TransactionKind` 枚举（8 种，唯一表示；DB/wire 边界的小写字符串映射经 `as_str`/`parse` 收口，serde 小写字符串序列化）+ kind→度量系数矩阵 + `convert_to_native` 本位币折算。
2. **kind 真源为 8 种**，与 `transactions.kind` 的 CHECK 约束（V001）一一对应：
   `income` / `expense` / `transfer` / `refund` / `buy` / `sell` / `dividend` / `split`。
3. **raw/native 分离语义唯一。** `amount_cents`（原始币种金额）与 `amount_native_cents`（本位币金额）在模块内语义唯一；`convert_to_native` 以**全局默认币种**（当前常量 `CNY`，未来读用户设置）为折算基准，**与账户币种无关**（避免跨账户漂移），正反向汇率兜底、缺汇率报错不静默混币种。MVP 阶段多币种汇率 1:1 保持不变。
4. **kind→度量矩阵为单一真源**，同时驱动「服务端聚合 SQL 片段」（`*_expr`）与「行级 Rust 助手」（`signed_amount`），两侧口径恒一致；修改任何口径只改矩阵一处。四个具名度量：

   | kind | account_flow | expense_net | income_net | refund_gross |
   |------|------|------|------|------|
   | income | + | 0 | + | 0 |
   | expense | − | + | 0 | 0 |
   | transfer | account_id=− / to_account_id=+ | 0 | 0 | 0 |
   | refund | + | − | 0 | + |
   | buy | − | 0 | 0 | 0 |
   | sell | + | 0 | 0 | 0 |
   | dividend | + | 0 | + | 0 |
   | split | 0 | 0 | 0 | 0 |

   - `account_flow`：某账户视角的现金出入（余额口径），transfer 按侧取号。
   - `expense_net`：支出净额 = 毛支出 − 退款；投资类（buy/sell）不计入经营收支。
   - `income_net`：收入净额 = 收入 + 分红（dividend 计入收入）。
   - `refund_gross`：退款毛额，独立成列，毛值/净值并存展示。
   - 净值关系恒等式一处定义：`expense_net = expense_gross − refund_gross`（月度汇总毛值三列由 `expense_gross_expr` 经恒等式导出，不另立度量）。

## 理由

1. **refund 双度量是"why"。** 退款对账户是现金回流（`account_flow=+`），对支出度量是冲减（`expense_net=−`），同时还要能单独看毛额（`refund_gross`）。矩阵按度量分别定义符号，三种需求各自成立，不再互相打架——这是单一"kind 符号"方案做不到的。
2. **投资不进经营报表。** buy/sell 属资本变动，`expense_net`/`income_net` 均为 0，避免投资活动扭曲支出/收入报表；dividend 作为收入计入 `income_net`；split 现金影响恒 0。
3. **折算以全局默认币种为基准。** 各账户交易统一折算到同一本位币，报表/预算/余额才能得出可比较的数字；若以账户币种为基准，跨账户汇总会出现口径漂移。折算收口一处后，未来汇率生效只需改模块内一处，不冒九处漂移。
4. **写入权威一处。** 交易行字段（含审计字段 created_at/updated_at/version/device_id）由 `insert_row`/`update_row` 统一生成，新增交易类型只改模块一处，不会漏掉某个写入路径。

## 代价

1. **行为保持重构，收益在"未来改动成本"而非当下功能。** 当前 MVP 多币种 1:1，矩阵与折算的差异暂不可见；真实收益在汇率生效、新增 kind 时兑现。
2. **命令层与领域模块之间存在接线转换。** `TransactionInput` ↔ `writer::Input`、`NormalizedTransaction` ↔ `writer::NormalizedRow` 需要字段映射（`commands/transactions/write.rs` 的 `to_writer_input`/`to_writer_row`），多一层薄转换。
3. **buy/sell 的行归一化仍留在投资层**（`prepare_buy`/`prepare_sell` 产出 `NormalizedTransaction` 后经 `to_writer_row` 落库）；持仓/卖出副作用仍与交易写入耦合，属 spec #52 明确的"候选 2"（交易类型行为内聚）未处理项。

## 替代方案

- **保留命令层各写入点的手写 SQL 与 CASE WHEN**：散落逻辑继续漂移，本次 spec 的根因不消除，放弃。
- **以账户币种为折算基准**：跨账户汇总口径漂移；且旧实现即此方案，已随接线删除（`commands::fx::convert_to_native`），放弃。
- **单一 kind 符号 + 特殊处理 refund**：三个度量对 refund 要求冲突，单一符号无法两全，放弃。
- **把 buy/sell 归一化也收进 Writer**：Writer 会反向依赖投资层（标的校验、持仓查询），破坏"领域模块不反向依赖命令层"的方向约束，放弃。

## 影响

- `transaction` 领域模块：`src-tauri/src/transaction/{mod,amount,writer}.rs`（+ 各自测试）。
- 消费方接线：余额（`db/balance.rs` 走 `account_flow_expr`）、报表（`reports` 域走毛值三列 + `expense_net`/`income_net`）、预算（`budget` 域走 `expense_net`）、定时引擎（`scheduled_transactions/engine` 改经 `writer::normalize` 落库）、批量导入（`commands/batch` 编排 + writer 落库）、创建/修改/买入卖出行（`commands/transactions`/`commands/investment`）。
- 删除：命令层旧 `normalize_transaction`/`row_to_normalized`（#61）、`commands::fx::exchange_rate`/`convert_to_native`（#60）、`read.rs::update_transaction_row`（#60）。
- 文档同步：`AGENTS.md` 修正 `transactions.kind` 为 8 种并指向模块接缝；`CONTEXT.md` 补充 Transaction Kind Mapping（8 种 + 度量矩阵）与 Amount Model（raw/native + 四度量）。
- 无 schema 变更、无迁移（V001 的 CHECK 约束本就含 8 种 kind）。
