# ADR 0001: 定时交易使用独立的 ScheduledTransaction + Occurrence 模型

- 状态：已接受
- 日期：2026-07-07
- 作者：Oz / Ledger 项目

## 背景

Ledger 需要支持三类定时/定期资金动作：分期付款、定期订阅、定时转账。这类动作的本质不是“一次性交易”，而是“按规则多次生成交易”。

在现有 V001 schema 中，`transactions` 表已经承担所有资金流水的职责。我们需要决定：

- 方案 A：把未来交易直接作为 `transactions` 记录预先写入，到日期时自动“生效”。
- 方案 B：引入独立的 `ScheduledTransaction`（计划）和 `Occurrence`（期次），到日期时才生成真正的 `Transaction`。

## 决策

采用方案 B：新增 `scheduled_transactions` 和 `scheduled_transaction_occurrences` 两张表，`transactions` 只保留实际已发生的资金流水。

## 理由

1. **概念边界清晰**
   - `transactions` 代表“已经发生、影响余额”的资金流水。
   - `ScheduledTransaction` 代表“用户同意在未来按规则付款/转账”的协议。
   - 两者混淆会让余额计算、预算统计、审计都变得复杂。

2. **支持单期管理**
   - 分期/订阅通常需要查看每一期的状态（待执行、已执行、失败）。
   - 如果只有未来交易记录，无法自然表达“第 3 期失败，第 4 期待执行”这样的状态。

3. **离线多设备同步安全**
   - 应用离线优先，设备 A 执行第 3 期后，设备 B 同步回来需要知道“第 3 期已执行，交易 ID 是 X”。
   - 独立的 `Occurrence` 表可以携带 `transaction_id` 和 `status`，天然避免两设备各自生成重复交易。

4. **失败与重试可控**
   - 失败发生在 `Occurrence` 层，可以标记为 `failed` 并等待用户手动重试。
   - 如果直接把失败状态写在 `Transaction` 里，会污染真实交易流水，且难以表达“计划继续”的语义。

5. **扩展性**
   - 后续支持单期跳过、改期、涨价、自动重试策略时，只需修改 `Occurrence` 或计划表，不影响核心交易模型。

## 代价

1. **查询更复杂**
   - 查“未来待付款”需要 JOIN `scheduled_transactions` + `scheduled_transaction_occurrences`。
   - 可以通过物化视图或聚合查询补偿。

2. **数据一致性需要维护**
   - 取消计划时，需要级联更新所有未执行 `Occurrence` 的状态为 `cancelled`。
   - 执行 occurrence 时需要两步：`status = processing` → 生成 `Transaction` → 回填 `transaction_id`。

3. **更多表和迁移**
   - 需要新增两张表和相应索引，以及未来窗口展开的应用层逻辑。

## 替代方案

- 方案 A（未来交易直接写入 `transactions`）：实现简单，但模糊了“计划”和“已发生交易”的边界，无法表达单期失败/重试，也不利于离线同步去重。
- 方案 C（纯规则驱动，不实例化 occurrence）：数据量最小，但多设备同步时容易重复执行，且无法支持单期状态查看。

## 影响

- 新增 `docs/model/scheduled-transactions/index.md` 数据模型文档。
- 新增 `CONTEXT.md` 中相关领域术语。
- 后续数据库迁移需要新增 `scheduled_transactions` 和 `scheduled_transaction_occurrences` 表，并复用现有 `device_id` / `version` / `updated_at` / `is_deleted` 同步字段。

## 结论

独立的 `ScheduledTransaction + Occurrence` 模型更适合 Ledger 离线优先、多设备同步、需要单期状态管理的场景。虽然增加了查询复杂度，但换来了更清晰的领域边界和更安全的执行语义。
