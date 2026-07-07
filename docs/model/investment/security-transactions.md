# security_transactions（证券交易扩展表）

一对一关联 transactions，记录证券/基金的专用字段。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| transaction_id | TEXT PK FK | 关联交易 ID（ON DELETE CASCADE） |
| instrument_id | TEXT FK | 关联金融工具 |
| action | TEXT | 交易动作（见下方枚举） |
| quantity | REAL | 数量变化（拆股/送股/分红可为 NULL） |
| price_cents | INTEGER | 成交单价（分），分红/拆股可为 NULL |
| fee_cents | INTEGER | 手续费/佣金（分） |

## 交易动作枚举

| 动作 | 含义 | quantity | price_cents |
|------|------|----------|-------------|
| `buy` | 买入 | 买入数量 | 成交单价 |
| `sell` | 卖出 | 卖出数量 | 成交单价 |
| `dividend` | 分红 | NULL | NULL |
| `split` | 拆股/送股 | 数量变化 | NULL |

## 设计说明

- 主键为 transaction_id，与 transactions 表一对一关联
- 现金部分由 transactions 表表达，账户余额计算无需额外 JOIN
- 分红/拆股等无资金变动时，transactions.amount_cents 为 0
- 通过 instrument_id 关联 instruments 表，避免 symbol / instrument_type 重复录入和潜在不一致
- 交易删除时级联删除（ON DELETE CASCADE），扩展表跟随主表生命周期
- 工具删除时受限（ON DELETE RESTRICT），防止证券交易记录孤立

## 被引用关系

- security_lots.buy_transaction_id → security_transactions.transaction_id（ON DELETE CASCADE）
- security_lot_sales.sell_transaction_id → security_transactions.transaction_id（ON DELETE CASCADE）

## 参考

- Migration：`src-tauri/migrations/V002__investment.sql`
