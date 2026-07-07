# security_lot_sales（批次卖出匹配表）

记录一笔卖出交易匹配了哪些 lot、各卖出多少、对应的已实现盈亏。

## 表结构

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | 匹配记录 UUID v7 |
| sell_transaction_id | TEXT FK | 关联卖出交易（ON DELETE CASCADE） |
| lot_id | TEXT FK | 关联被卖出的批次（ON DELETE CASCADE） |
| quantity | REAL | 卖出的该批次数量 |
| cost_per_unit_cents | INTEGER | 卖出时该批次单位成本（分） |
| realized_pnl_cents | INTEGER | 已实现盈亏（分），已扣除卖出手续费 |
| currency_code | TEXT FK | 币种 |
| created_at | TEXT | 创建时间 |

## 设计说明

- 记录一笔卖出交易匹配了哪些 lot、各卖出多少、对应的已实现盈亏
- realized_pnl_cents 是已实现盈亏的审计来源
- 从 lot 重新计算持仓的依据
- 卖出交易或批次删除时级联删除（ON DELETE CASCADE），匹配记录跟随交易和批次生命周期

## 索引

- `idx_security_lot_sales_lot`：(lot_id) 用于查询某批次的所有卖出记录

## 参考

- Migration：`src-tauri/migrations/V002__investment.sql`
