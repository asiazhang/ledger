# security_lots（持仓批次表）

每笔买入交易产生一个 lot，记录独立的成本 basis。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 批次 UUID v7 |
| account_id | TEXT FK | 关联账户 |
| instrument_id | TEXT FK | 关联金融工具 |
| buy_transaction_id | TEXT FK | 关联买入交易（ON DELETE CASCADE） |
| initial_quantity | REAL | 买入数量 |
| remaining_quantity | REAL | 剩余数量（卖出后扣减） |
| cost_per_unit_cents | INTEGER | 单位成本（分），已含买入手续费摊薄 |
| currency_code | TEXT FK | 成本币种 |
| created_at | TEXT | 创建时间 |
| updated_at | TEXT | 最后更新时间 |
| version | INTEGER | 版本号 |
| device_id | TEXT | 设备标识 |
| UNIQUE | (account_id, instrument_id, buy_transaction_id) | 同一买入交易唯一 |

## 设计说明

- 每笔买入交易产生一个 lot，记录独立的成本 basis
- 支持 FIFO / LIFO / 平均成本 / 指定 lot 等卖出匹配规则
- 卖出时通过 security_lot_sales 记录匹配的批次及已实现盈亏，并扣减 remaining_quantity
- 拆股/送股等公司行为需要应用层调整所有相关 lot 的 quantity 和 cost_per_unit_cents
- 唯一约束保证同一买入交易只产生一个批次
- 买入交易删除时级联删除（ON DELETE CASCADE），批次跟随交易生命周期
- 账户和工具删除时受限（ON DELETE RESTRICT），防止持仓批次孤立

## 索引

- `idx_security_lots_active_covering`：部分覆盖索引 (account_id, instrument_id, currency_code, remaining_quantity, cost_per_unit_cents, updated_at) WHERE remaining_quantity > 0，优化 v_holdings 聚合查询
- `idx_security_lots_buy_transaction`：(buy_transaction_id) 用于级联删除
- `idx_security_lots_sync`：(updated_at, device_id) 用于同步查询

## 被引用关系

- security_lot_sales.lot_id → security_lots.id（ON DELETE CASCADE）

## 参考

- Migration：`src-tauri/migrations/V002__investment.sql`
