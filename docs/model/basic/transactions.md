# transactions（交易表）

核心表，记录所有资金流动。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 交易 UUID v7 |
| kind | TEXT | 交易类型（见下方枚举） |
| amount_cents | INTEGER | 原始币种金额（分） |
| currency_code | TEXT FK | 原始币种代码 |
| amount_native_cents | INTEGER | 本位币金额（分，当前 1:1，预留多币种换算） |
| account_id | TEXT FK | 关联账户（支出/收入/转出账户） |
| to_account_id | TEXT FK | 转入账户（仅 transfer 时必填） |
| category_id | TEXT FK | 关联分类（转账通常为空） |
| refund_of_transaction_id | TEXT FK | 退款关联的原始支出交易 ID |
| note | TEXT | 交易备注（可选） |
| date | TEXT | 交易日期（ISO 8601 格式 YYYY-MM-DD） |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 最后修改时间 |
| version | INTEGER | 版本号 |
| device_id | TEXT | 设备标识 |
| is_deleted | INTEGER | 软删除标志 |

## 交易类型枚举

| 类型 | 含义 | 资金流向 |
|------|------|----------|
| `income` | 收入 | 增加 account_id 账户余额 |
| `expense` | 支出 | 减少 account_id 账户余额 |
| `transfer` | 转账 | 从 account_id 转出，加到 to_account_id |
| `refund` | 退款 | 关联原 expense 交易（refund_of_transaction_id），退回原账户 |
| `buy` | 买入证券/基金 | 减少账户现金，由 security_transactions 扩展记录持仓变化 |
| `sell` | 卖出证券/基金 | 增加账户现金，由 security_transactions 扩展记录持仓变化 |
| `dividend` | 现金分红 | 增加账户现金，security_transactions 记录对应标的 |
| `split` | 拆股/送股 | 不改变账户现金，仅通过 security_transactions 调整持仓数量 |

## 设计说明

- 交易是资金流动的核心记录，所有投资相关交易在 transactions 表中只表达现金部分
- 投资交易的证券/基金专用字段由 security_transactions 扩展表记录
- 多币种预留：同时存储 amount_cents（原始币种）和 amount_native_cents（本位币），当前 1:1 相等
- 退款通过 refund_of_transaction_id 关联原支出交易，复用原支出交易的分类
- 关联的账户硬删时受限（ON DELETE RESTRICT），防止交易孤立；转入账户和分类删除时置空

## 索引

- `idx_transactions_date`：(date) 按日期查询
- `idx_transactions_account`：(account_id) 按账户查询
- `idx_transactions_category`：(category_id) 按分类查询
- `idx_transactions_refund`：(refund_of_transaction_id) 查询退款关联
- `idx_transactions_sync`：(updated_at, device_id) 同步查询
- `idx_transactions_deleted`：(is_deleted, updated_at) 软删除查询

## 被引用关系

- security_transactions.transaction_id → transactions.id（ON DELETE CASCADE）
- transactions.refund_of_transaction_id → transactions.id（ON DELETE SET NULL，自引用）

## 参考

- Migration：`src-tauri/migrations/V001__initial.sql`
