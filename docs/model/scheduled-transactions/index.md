# Scheduled Transactions 模块

> 本模块包含三类定时/定期交易模型：分期计划、订阅、定时转账。采用“核心表 + 扩展表”设计：通用字段和生命周期放在核心表，类型特有字段放在扩展表。

## 文档结构

- `index.md`：本文件，模块概述与共享规则
- `installment.md`：分期计划特有字段与行为
- `subscription.md`：订阅特有字段与行为
- `scheduled-transfer.md`：定时转账特有字段与行为

## 设计原则

1. **核心 + 扩展**：通用字段集中在 `scheduled_transactions` 和 `scheduled_transaction_occurrences`，类型特有字段放在各自的扩展表。
2. **类型边界清晰**：扩展表让分期、订阅、定时转账的约束和字段互不干扰。
3. **期次统一**：使用一张 `scheduled_transaction_occurrences` 表，通过 `scheduled_transaction_id` 关联核心表，避免多态外键和三张重复期次表。
4. **交易生成统一**：每种计划到期时都生成 V001 的 `transactions` 记录。
5. **UI 仍可独立**：应用层根据 `scheduled_transactions.kind` 分发到不同的 UI 和命令，数据模型与表现层解耦。

## 实体关系

```
scheduled_transactions (1) ──< (*) scheduled_transaction_occurrences
scheduled_transactions (1) ──< (1) installment_plans
scheduled_transactions (1) ──< (1) subscription_plans
scheduled_transactions (1) ──< (1) scheduled_transfer_plans

scheduled_transaction_occurrences (0..1) ──> (1) Transaction
```

## `scheduled_transactions`（核心计划表）

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `id` | TEXT | 是 | UUID v7，全局唯一主键 |
| `kind` | TEXT | 是 | `installment` / `subscription` / `scheduled_transfer` |
| `status` | TEXT | 是 | `active` / `paused` / `cancelled` / `completed` |
| `account_id` | TEXT | 是 | 扣款/转出账户，FK → `accounts(id)` |
| `category_id` | TEXT | 否 | 交易分类，FK → `categories(id)` |
| `amount_cents` | INTEGER | 是 | 每期金额（分） |
| `currency_code` | TEXT | 是 | 币种，FK → `currencies(code)` |
| `recurrence_type` | TEXT | 是 | 周期类型：`daily` / `weekly` / `monthly` / `yearly` |
| `recurrence_interval` | INTEGER | 是 | 间隔，默认 1 |
| `recurrence_day` | INTEGER | 否 | 具体日期/星期；如每月 1 日、每周一 |
| `start_date` | TEXT | 是 | 第一期执行日期，YYYY-MM-DD |
| `note` | TEXT | 否 | 备注 |
| `created_at` | TEXT | 是 | 创建时间，UTC ISO 8601 |
| `updated_at` | TEXT | 是 | 最后修改时间，UTC ISO 8601 |
| `version` | INTEGER | 是 | 版本计数，每次修改 +1 |
| `device_id` | TEXT | 是 | 创建设备/最后修改设备标识 |
| `is_deleted` | INTEGER | 是 | 软删除标志，0/1 |

## 扩展表（每类计划一张）

| 扩展表 | 关联 | 类型特有字段 |
|---|---|---|
| `installment_plans` | `scheduled_transaction_id` PK/FK | `merchant_id`, `total_amount_cents`, `total_occurrences` |
| `subscription_plans` | `scheduled_transaction_id` PK/FK | `merchant_id` |
| `scheduled_transfer_plans` | `scheduled_transaction_id` PK/FK | `to_account_id`, `total_occurrences` |

详见 `installment.md` / `subscription.md` / `scheduled-transfer.md`。

## `scheduled_transaction_occurrences`（统一期次表）

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `id` | TEXT | 是 | UUID v7，全局唯一主键 |
| `scheduled_transaction_id` | TEXT | 是 | 所属计划，FK → `scheduled_transactions(id)` |
| `scheduled_date` | TEXT | 是 | 应执行日期，YYYY-MM-DD |
| `status` | TEXT | 是 | `pending` / `processing` / `completed` / `failed` / `cancelled` |
| `transaction_id` | TEXT | 否 | 生成的交易 ID，完成后回填，FK → `transactions(id)` |
| `amount_cents` | INTEGER | 是 | 该期实际金额（分） |
| `created_at` | TEXT | 是 | 创建时间，UTC ISO 8601 |
| `updated_at` | TEXT | 是 | 最后修改时间，UTC ISO 8601 |
| `version` | INTEGER | 是 | 版本计数，每次修改 +1 |
| `device_id` | TEXT | 是 | 创建设备/最后修改设备标识 |
| `is_deleted` | INTEGER | 是 | 软删除标志，0/1 |

## 状态机

### 计划状态

```
active ──pause──> paused
paused ──resume──> active
active ──cancel──> cancelled
paused ──cancel──> cancelled
active ──完成所有期次──> completed
```

- `cancelled` 不会删除已生成的 `Transaction`。
- `paused` 不会撤销已生成的期次或交易。

### 期次状态

```
pending ──execute──> processing ──success──> completed (transaction_id 回填)
                              └─failure──> 返回错误，状态不变（可重试）
pending ──cancel plan──> cancelled
```

- MVP 不支持 `skipped` 状态（单期跳过）。
- `failed` 状态存在于 CHECK 约束与枚举（`OccurrenceStatus::Failed`），但当前实现中**没有写入 `status='failed'` 的路径**：执行失败（如非默认币种缺汇率、CAS 冲突、落库错误）时直接返回 `AppError`，期次保持 `pending`（或 `processing`），可再次手动执行重试。状态机文档中的 `failed` 为 schema 预留，尚未被代码使用。

## 周期规则

- MVP 使用显式字段：`recurrence_type` / `recurrence_interval` / `recurrence_day`。
- 不引入 RRULE 等通用表达式。
- 时间只精确到日期，不因周末/节假日顺延。
- 已发生/已生成的期次必须持久化。
- 未来期次只预生成有限窗口（如未来 6 个月或 12 期），耗尽时自动再展开一批。
- 订阅和无限期定时转账的窗口持续向前滚动。

## 失败策略

- MVP 阶段实现为“执行失败即返回错误，期次状态不变，由用户手动重试”（失败不落 `failed` 状态，见上）。
- 不自动重试、不自动跳过、不产生滞纳金。
- 理由：离线优先场景下，自动重试容易在多设备间产生重复执行；手动重试让用户明确控制资金流出。

## 离线同步与并发控制

- 核心表、扩展表、期次表均携带 `device_id` / `version` / `updated_at` / `is_deleted`，复用现有离线同步冲突解决策略。
- 执行期次时采用基于 `version` 的 CAS：
  1. 更新 `status = processing` 且 `version` + 1，只有更新成功的设备才执行。
  2. 生成 `Transaction`。
  3. 回填 `transaction_id`，`status = completed`，`version` 再 +1。
- 同步时若发现 `transaction_id` 已存在，跳过执行，避免多设备重复扣款或转账。

## 跨类型查询

核心表统一后，跨类型查询只需要 JOIN 核心表和期次表，扩展表只在需要类型特有字段时 JOIN：

```sql
-- 所有待执行的定时交易（不含类型特有字段）
SELECT st.id, st.kind, st.account_id, st.amount_cents, sto.scheduled_date, sto.status
FROM scheduled_transactions st
JOIN scheduled_transaction_occurrences sto ON st.id = sto.scheduled_transaction_id
WHERE sto.status = 'pending' AND sto.scheduled_date <= ?
```

```sql
-- 待执行的分期计划（需要分期特有字段）
SELECT st.id, st.account_id, st.amount_cents, sto.scheduled_date,
       ip.total_amount_cents, ip.total_occurrences
FROM scheduled_transactions st
JOIN scheduled_transaction_occurrences sto ON st.id = sto.scheduled_transaction_id
JOIN installment_plans ip ON st.id = ip.scheduled_transaction_id
WHERE st.kind = 'installment' AND sto.status = 'pending' AND sto.scheduled_date <= ?
```

## 索引建议

- `scheduled_transactions(account_id)`
- `scheduled_transactions(kind, status)`
- `scheduled_transactions(updated_at, device_id)`
- `scheduled_transaction_occurrences(scheduled_transaction_id, scheduled_date)`
- `scheduled_transaction_occurrences(scheduled_date, status)`
- `scheduled_transaction_occurrences(transaction_id)` UNIQUE
- `scheduled_transaction_occurrences(updated_at, device_id)`
- 各扩展表：`scheduled_transaction_id`（主键/唯一索引）

## MVP 范围

**支持**：
- 创建分期计划、订阅、定时转账。
- 周期：日/周/月/年，固定间隔。
- 整体暂停、恢复、取消计划。
- 手动重试失败的期次（重试即再次执行该期次；失败不改变期次状态）。
- 定时转账支持一次性（`total_occurrences = 1`）或无限循环。

**不支持**：
- 单期取消/跳过/修改金额。
- 自动重试、滞纳金、节假日顺延。
- subscription 自动结束日期、中途涨价。
- 复杂 RRULE 表达式。
- 在 `Transaction` 表中新增通用 `plan_id` 字段（`merchant_id` 属 ADR-0028 商户维度，与分类同款可选字段，见核心交易域词汇表）。

## 后续扩展方向

- 各类型特有扩展见 `installment.md` / `subscription.md` / `scheduled-transfer.md`。
- 如果未来类型差异继续扩大，可把核心表进一步拆分为独立表；当前阶段核心 + 扩展表在简洁与独立之间取得平衡。
