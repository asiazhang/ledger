# 订阅（Subscription）

> 订阅是 `ScheduledTransaction` 的一种业务形态。通用字段、状态机、周期规则、同步与并发控制见 `index.md`。

## 业务定义

- 按周期持续扣款，直到用户手动取消或暂停的资金安排。
- 每次触发时生成一条 `Transaction`（`kind = expense`）。
- MVP 阶段没有结束日期，也没有最大期数限制。

## 扩展表 `subscription_plans`

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `scheduled_transaction_id` | TEXT | 是 | 主键/外键，FK → `scheduled_transactions(id)`，且 `scheduled_transactions.kind = 'subscription'` |
| `counterparty` | TEXT | 否 | 订阅服务商，如 Netflix、爱奇艺、iCloud |

## 状态转换

- 计划状态：`active` → `paused` / `cancelled`
- MVP 阶段不使用 `completed`，因为订阅没有自然结束点。
- 期次状态：`pending` → `processing` → `completed` / `failed`

## 生成交易规则

| 源字段 | 目标 `Transaction` 字段 |
|---|---|
| `scheduled_transactions.account_id` | `account_id` |
| `scheduled_transactions.amount_cents` | `amount_cents` / `amount_native_cents`（本位币，当前 1:1） |
| `scheduled_transactions.currency_code` | `currency_code` |
| `scheduled_transactions.category_id` | `category_id` |
| `subscription_plans.counterparty` | 复制到 `note` 或作为展示字段 |
| 期次 `scheduled_date` | `date` |
| 固定值 | `kind = 'expense'` |

## 周期展开规则

- 订阅没有结束日期，未来期次窗口持续向前滚动。
- 已发生/已生成的期次必须持久化。
- 未来期次只预生成有限窗口（如未来 6 个月或 12 期），耗尽时自动再展开一批。

## 边界与约束

- `amount_cents` 固定，MVP 不支持中途涨价。
- 取消后不再生成新期次，已生成 `Transaction` 不受影响。
- 暂停只停止生成新期次，不影响已生成的期次或交易。
- 创建订阅时，必须同步创建 `subscription_plans` 扩展记录。

## 后续可扩展方向

- 结束日期：`end_date` 字段（可放在核心表或扩展表），到期后自动停止。
- 涨价记录：单独的价格变更表，支持按日期追溯每期应付金额。
- 试用期/促销期：首期免费或折扣。
- 自动续费开关：与 `paused` 状态分离的明确开关。
