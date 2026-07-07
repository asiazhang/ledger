# 已实现盈亏报表需求

> 记录基于 `security_lot_sales` 表生成已实现盈亏（Realized P&L）报表的前后端需求。

## 问题

`security_lot_sales` 表已记录每次卖出的匹配和盈亏，但前端没有展示入口。

## 优化方向

1. 新增报表页面或卡片，按账户/标的/时间维度展示：
   - 总已实现盈亏。
   - 每笔卖出对应的 lot 匹配明细。
   - 按年度聚合，便于税务申报。
2. 后端命令 `realized_pnl_summary` 从 `security_lot_sales` 聚合。

## 关联

- `src/views/ReportsView.vue` 或新增 `src/views/InvestmentsView.vue`
- `src-tauri/src/commands.rs`
- `src-tauri/migrations/V002__investment.sql` 的 `security_lot_sales` 表
