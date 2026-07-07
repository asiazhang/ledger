# v_holdings（持仓视图）

由 security_lots 实时聚合，不作为主数据存储，避免与交易流水不一致。

## 输出字段

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT | 账户-工具-币种组合键 |
| account_id | TEXT | 账户 ID |
| instrument_id | TEXT | 金融工具 ID |
| quantity | REAL | 当前持仓数量 |
| cost_basis_cents | INTEGER | 成本基础（账户币种） |
| cost_currency_code | TEXT | 成本币种 |
| latest_price_cents | INTEGER | 最新价格 |
| latest_price_currency_code | TEXT | 价格币种 |
| market_value_cents | INTEGER | 市值（账户币种，可能为 NULL） |
| unrealized_pnl_cents | INTEGER | 未实现盈亏（账户币种，可能为 NULL） |
| updated_at | TEXT | 最后更新时间 |

## 计算逻辑

- 聚合 security_lots 中 remaining_quantity > 0 的记录
- 按 (account_id, instrument_id, currency_code) 分组
- 排除软删除账户的持仓（WHERE account_id IN (SELECT id FROM accounts WHERE is_deleted = 0)）
- 关联 market_prices 获取最新价格
- 通过 exchange_rates 进行币种折算（支持正向和反向汇率）
- 同币种时汇率视为 1（无需查表）
- 当无法取到行情或汇率时，market_value_cents / unrealized_pnl_cents 为 NULL

## 市值计算

```
market_value_cents = quantity * price_cents * exchange_rate
```

其中 exchange_rate 的优先级：
1. 价格币种 = 账户币种：汇率为 1
2. 正向汇率存在：使用 base_code = 价格币种, quote_code = 账户币种
3. 反向汇率存在：使用 base_code = 账户币种, quote_code = 价格币种，取倒数
4. 均不存在：NULL

## 未实现盈亏计算

```
unrealized_pnl_cents = market_value_cents - cost_basis_cents_in_account_currency
```

其中 cost_basis_cents_in_account_currency 的折算逻辑与市值相同，使用 lot 成本币种到账户币种的汇率。

## 优化索引

- `idx_security_lots_active_covering`：部分覆盖索引 (account_id, instrument_id, currency_code, remaining_quantity, cost_per_unit_cents, updated_at) WHERE remaining_quantity > 0
- 前三列对齐 GROUP BY 提供有序扫描免排序
- 后三列覆盖 SUM(remaining_quantity) / SUM(remaining_quantity * cost_per_unit_cents) / MAX(updated_at) 免回表

## 参考

- Migration：`src-tauri/migrations/V002__investment.sql`
