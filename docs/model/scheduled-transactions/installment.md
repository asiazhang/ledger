# 分期计划（Installment Plan）

> 分期计划是 `ScheduledTransaction` 的一种业务形态。通用字段、状态机、周期规则、同步与并发控制见 `index.md`。

## 业务定义

- 在固定期数内、按固定周期偿还一笔已知总金额的资金安排。
- 已还金额和已还期数由 `scheduled_transaction_occurrences` 的 `completed` 状态实时汇总。
- 每次触发时生成一条 `Transaction`（`kind = expense`）。

## 扩展表 `installment_plans`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `scheduled_transaction_id` | TEXT | 是 | 主键/外键，FK → `scheduled_transactions(id)`，且 `scheduled_transactions.kind = 'installment'` |
| `merchant_id` | TEXT | 否 | 商户引用，FK → `merchants(id)`（硬删置空；每期生成交易时复制到流水，issue #190 / ADR-0028） |
| `total_amount_cents` | INTEGER | 是 | 分期总金额（分） |
| `total_occurrences` | INTEGER | 是 | 分期总期数 |

## 金额计算规则

1. 基准每期金额 = `floor(total_amount_cents / total_occurrences)`
2. 尾差 = `total_amount_cents - base_amount_cents * total_occurrences`
3. 前 `total_occurrences - 1` 期金额为 `base_amount_cents`
4. 最后一期金额为 `base_amount_cents + 尾差`

该计算在创建/修改计划时确定，结果写入各期次的 `amount_cents`。

## 状态转换

- 计划状态：`active` → `paused` / `cancelled` / `completed`（所有期次执行完毕）
- 期次状态：`pending` → `processing` → `completed`；执行失败时返回错误、期次状态不变（`failed` 状态为 schema 预留，见 index.md）

## 生成交易规则

| 源字段 | 目标 `Transaction` 字段 |
|---|---|
| `scheduled_transactions.account_id` | `account_id` |
| 期次 `amount_cents` | `amount_cents` / `amount_native_cents`（本位币，当前 1:1） |
| `scheduled_transactions.currency_code` | `currency_code` |
| `scheduled_transactions.category_id` | `category_id` |
| `installment_plans.merchant_id` | `merchant_id`（每期复制计划商户到流水，issue #190 / ADR-0028） |
| 期次 `scheduled_date` | `date` |
| 固定值 | `kind = 'expense'` |

## 汇总查询

- 已还期数 = `COUNT(scheduled_transaction_occurrences WHERE status = 'completed')`
- 已还金额 = `SUM(scheduled_transaction_occurrences.amount_cents WHERE status = 'completed')`
- 剩余金额 = `total_amount_cents - 已还金额`

## 边界与约束

- `total_occurrences` >= 1。
- `total_amount_cents` >= `total_occurrences`（保证每期至少 1 分）。
- 取消计划时，所有 `pending` 期次状态变为 `cancelled`，已生成 `Transaction` 不受影响。
- 创建分期计划时，必须同步创建 `installment_plans` 扩展记录。

## 后续可扩展方向

- 提前还款：一次性还清剩余金额，生成额外 `Transaction`。
- 部分还款：某期支付金额高于/低于计划金额，需要调整剩余期次金额。
- 逾期罚金：失败一定时间后生成额外 `expense`。
- 利率/手续费：在总金额之外单独记录利息或手续费。
