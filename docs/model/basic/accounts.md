# accounts（账户表）

管理用户的各类金融资产账户。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 账户 UUID v7 |
| name | TEXT | 账户名称（如「招行储蓄卡」「现金」） |
| type | TEXT | 账户类型（见下方枚举） |
| currency_code | TEXT FK | 账户本位币代码 |
| initial_balance_cents | INTEGER | 初始余额（分） |
| created_at | TEXT | 创建时间（UTC ISO 8601） |
| updated_at | TEXT | 最后修改时间（用于 LWW） |
| version | INTEGER | 版本号（每次修改 +1） |
| device_id | TEXT | 创建/最后修改设备标识 |
| is_deleted | INTEGER | 软删除标志（0/1） |
| is_hidden | INTEGER | 隐藏标志（0/1），黑洞账户专用（V001） |

> 黑洞账户（`is_hidden=1`，如「无(CNY)」）用于承接「资金账户=无」的导入交易：交易照常写入并出现在列表/报表，但账户对用户侧列表/余额/下拉选择器隐藏（IPC `list_accounts` / `list_account_balances` 过滤 `is_hidden=0`；HTTP API 返回完整列表含 `is_hidden` 字段，供 AI 对账）。种子见 `V004__seed_defaults.sql`。

## 账户类型枚举

| 类型 | 含义 | 余额特征 |
|------|------|----------|
| `cash` | 现金（钱包、零钱） | 余额非负 |
| `bank` | 银行账户（储蓄卡、工资卡、活期、定期、公积金等） | 余额非负 |
| `credit` | 信用卡、花呗、白条等信用支付账户 | 余额可为负（表示欠款） |
| `ewallet` | 电子钱包（微信钱包、支付宝余额等） | 余额非负 |
| `investment` | 投资账户（股票、基金、债券、ETF 等证券资金账户） | 余额非负 |
| `debt` | 负债账户（房贷、车贷、消费贷等） | 余额为负（表示尚未偿还） |
| `receivable` | 借出款/应收款账户 | 余额为正（表示对方尚未归还） |
| `other` | 其他账户（押金、公司垫付、自定义账户等） | 兜底类型 |

## 设计说明

- 每个账户有独立的本位币（currency_code），由 currencies 表定义
- 账户余额由 initial_balance_cents 加上所有关联 transactions 的金额变化计算得出
- 软删除通过 is_deleted 实现，不物理删除数据，保证同步一致性
- 投资账户的证券持仓通过 security_lots 表单独管理，账户余额仅表示现金部分

## 索引

- `idx_accounts_sync`：(updated_at, device_id) 用于同步查询

## 被引用关系

- transactions.account_id → accounts.id（ON DELETE RESTRICT）
- transactions.to_account_id → accounts.id（ON DELETE SET NULL）
- security_lots.account_id → accounts.id（ON DELETE RESTRICT）
- scheduled_transactions.account_id → accounts.id（计划扣款/转出账户）
- scheduled_transfer_plans.to_account_id → accounts.id（计划转账转入账户）

## 参考

- Migration：`src-tauri/migrations/V001__initial.sql`
