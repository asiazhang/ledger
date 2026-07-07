# ADR 0002: 按业务类型拆分独立的定时/定期交易模型

- 状态：已接受
- 日期：2026-07-07
- 作者：Oz / Ledger 项目

## 背景

在 ADR 0001 中，我们决定使用独立的 `ScheduledTransaction + Occurrence` 模型来替代“把未来交易直接塞进 `transactions` 表”的方案。接下来需要进一步决定：这三类业务（分期、订阅、定时转账）在数据层面是共用一张表，还是各自独立成表。

候选方案：

- 方案 A：单张 `scheduled_transactions` 表，用 `kind` 字段区分三种业务。
- 方案 B：核心表 + 扩展表（`scheduled_transactions` + `installment_plans` / `subscription_plans` / `scheduled_transfer_plans`）。
- 方案 C：完全独立模型（`installment_plans` + `installment_occurrences`、`subscriptions` + `subscription_occurrences`、`scheduled_transfers` + `scheduled_transfer_occurrences`）。

## 决策

采用方案 C：完全独立模型。每类业务有自己的计划表和期次表。

## 理由

1. **UI 与数据模型对齐**
   - 分期、订阅、定时转账的 UI 设计本身就按类型独立。数据模型与 UI 同构，减少认知摩擦。
   - 每个类型的字段、约束、校验可以直接写在各自的表上，避免“某些字段只在 kind = X 时必填”的复杂 CHECK。

2. **类型差异真实存在**
   - 分期有 `total_amount_cents` / `total_occurrences`。
   - 订阅有 `counterparty`，没有结束日期。
   - 定时转账有 `to_account_id`，而不是 `counterparty`。
   - 这些差异不是简单的“字段不同”，而是业务语义不同。独立表能表达得更清楚。

3. **避免单表膨胀**
   - 单张大表会堆积大量可空字段，后续每新增一种类型都会让表更宽、更难理解。
   - 独立表让每种类型只关心自己的字段，查询和维护都更聚焦。

4. **扩展互不干扰**
   - 后续为 subscription 加 `end_date`、涨价记录，为 installment 加提前还款，都不会影响其他类型的表。

## 代价

1. **表数量翻倍**
   - 从 2 张表（`scheduled_transactions` + `occurrences`）变成 6 张表（3 计划 + 3 期次）。
   - 迁移、索引、同步逻辑需要写三份，但结构相同，可以通过代码生成或共享工具函数减少重复。

2. **跨类型查询需要 UNION**
   - “所有账户的 upcoming payments”需要 UNION 三对表。
   - 这种查询只发生在 dashboard/提醒等场景，不影响单类型业务流程。

3. **抽象层需要额外设计**
   - 应用层可以通过 trait / interface 抽象共同的“计划 + 期次”行为，避免三份重复业务逻辑。

## 替代方案

- 方案 A：单表实现简单，但字段可空多、CHECK 约束复杂，且 UI 独立设计时容易造成数据层与表现层不匹配。
- 方案 B：核心 + 扩展表在概念上优雅，但核心表仍保留 `kind`，期次表仍需要多态或集中引用；对于三种差异明显的业务，独立表更直接。

## 影响

- `docs/model/scheduled-transactions/index.md` 更新为独立的六表结构。
- `CONTEXT.md` 中 `ScheduledTransaction` 术语被拆分为 `InstallmentPlan`、`Subscription`、`ScheduledTransfer`。
- 后续数据库迁移需要新增六张表，以及各自的索引和同步字段。
- 应用层需要为三种类型设计各自的 UI 和命令，同时抽象共同的执行引擎。

## 结论

虽然独立表会增加表数量，但换来了更清晰的类型边界、更简单的约束和与 UI 更自然的对应关系。对于分期、订阅、定时转账这种差异明显的业务，完全独立模型是当前阶段更合理的选择。
