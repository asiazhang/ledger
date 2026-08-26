# 定时转账（Scheduled Transfer）

> 定时转账是 `ScheduledTransaction` 的一种业务形态。通用字段、状态机、周期规则、同步与并发控制见 `index.md`。

## 业务定义

- 在预定日期从用户一个账户向另一个账户转出固定金额的资金安排。
- 可以是一次性（只执行一期），也可以是周期性（循环执行）。
- 每次触发时生成一条 `Transaction`（`kind = transfer`）。

## 扩展表 `scheduled_transfer_plans`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `scheduled_transaction_id` | TEXT | 是 | 主键/外键，FK → `scheduled_transactions(id)`，且 `scheduled_transactions.kind = 'scheduled_transfer'` |
| `to_account_id` | TEXT | 是 | 转入账户，FK → `accounts(id)` |
| `total_occurrences` | INTEGER | 否 | 总期数；为空表示无限循环，为 1 表示一次性定时转账，为 N 表示固定 N 期 |

## 状态转换

- 计划状态：`active` → `paused` / `cancelled` / `completed`（固定期数全部执行完）
- 一次性定时转账（`total_occurrences = 1`）执行后状态变为 `completed`。
- 无限循环的定时转账不使用 `completed` 状态。
- 期次状态：`pending` → `processing` → `completed`；执行失败时返回错误、期次状态不变（`failed` 状态为 schema 预留，见 index.md）

## 生成交易规则

| 源字段 | 目标 `Transaction` 字段 |
|---|---|
| `scheduled_transactions.account_id` | `account_id`（转出账户） |
| `scheduled_transfer_plans.to_account_id` | `to_account_id`（转入账户） |
| 期次 `amount_cents` | `amount_cents` / `amount_native_cents`（本位币，当前 1:1） |
| `scheduled_transactions.currency_code` | `currency_code` |
| `scheduled_transactions.category_id` | `category_id`（可选） |
| 期次 `scheduled_date` | `date` |
| 固定值 | `kind = 'transfer'` |

## 金额与期数规则

- 每期金额 = `scheduled_transactions.amount_cents`。
- `total_occurrences` 为空：无限循环，直到用户取消或暂停。
- `total_occurrences` = 1：一次性定时转账，最常见场景。
- `total_occurrences` = N：固定 N 期，全部完成后计划状态变为 `completed`。

## 边界与约束

- `to_account_id` 不能与 `account_id` 相同（转出账户不能等于转入账户）。
- 如果 `total_occurrences` 为 N，则必须 >= 1。
- 取消计划时，所有 `pending` 期次状态变为 `cancelled`，已生成 `Transaction` 不受影响。
- 暂停只停止生成新期次，不影响已生成的期次或交易。
- 创建定时转账时，必须同步创建 `scheduled_transfer_plans` 扩展记录。

## 后续可扩展方向

- 跨币种转账：根据 `exchange_rates` 折算 `amount_native_cents`。
- 转账附言/备注：从计划 `note` 复制到 `Transaction.note`。
- 条件触发：余额高于阈值时才执行（与简单定时区分开）。
- 定时收款：反向流程，从其他账户收款到本账户（涉及 `kind = income`）。
