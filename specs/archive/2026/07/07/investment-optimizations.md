# 投资模块后续优化需求

> 记录 V002 投资 schema 进一步可优化的方向。当前已落地：instrument_id 外键关联、`security_lots` 仓位批次表、`security_lot_sales` 卖出匹配表、`v_holdings` 视图。以下需求按优先级排列，供后续迭代参考。

---

## P0：市场价格表（market_prices） ✅

**问题**  
V002 注释中提到 `market_prices` 但尚未实现。没有市场价格就无法计算未实现盈亏、持仓市值和收益率。

**优化方向**  
新增 `market_prices` 表，支持按 instrument + 日期记录收盘价/最新价：

```sql
CREATE TABLE IF NOT EXISTS market_prices (
    id              TEXT PRIMARY KEY,
    instrument_id   TEXT NOT NULL REFERENCES instruments(id),
    price_cents     INTEGER NOT NULL,        -- 最新/收盘价（分）
    currency_code   TEXT NOT NULL,           -- 报价币种
    priced_at       TEXT NOT NULL,           -- 日期或时间戳
    source          TEXT,                    -- 数据来源（如 yahoo、manual）
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    version         INTEGER NOT NULL DEFAULT 1,
    device_id       TEXT NOT NULL,
    UNIQUE(instrument_id, priced_at)
);
```

**关联**  
`v_holdings` 可扩展 JOIN `market_prices` 计算 `market_value_cents` 和 `unrealized_pnl_cents`。

---

## P0：投资多币种折算 ✅

**问题**  
V001 中 `transactions.amount_native_cents` 目前为 1:1，注释也写明“预留多币种换算”。对于美股、港股等账户币种与标的币种不同的场景，直接 1:1 会导致持仓成本和盈亏完全无意义。

**优化方向**  
1. 在写入投资交易时，根据交易日期查询汇率，将 `amount_cents` 折算到账户本位币，写入 `amount_native_cents`。  
2. 汇率表 `exchange_rates` 已存在于 V001，但目前没有使用。需要补充汇率获取策略（手动录入、外部 API、收盘汇率）。  
3. `security_lots.cost_per_unit_cents` 应明确是“标的币种成本”还是“账户本位币成本”，建议统一用账户本位币存储，便于聚合。

**关联**  
`src-tauri/src/commands.rs` 的 `create_transaction` 和未来的 `create_security_transaction` 需要处理汇率。

---

## P1：投资交易 UI

该需求较大，已单独记录在 `specs/investment-transaction-ui.md` 中。本文件只保留 schema 层与数据层面的优化方向。

---

## P1：已实现盈亏报表

该需求较大，已单独记录在 `specs/realized-pnl-report.md` 中。

---

## 非功能性优化

### 1. 索引补充
- 为 `security_transactions(instrument_id)`、`security_transactions(account_id 透過 transactions JOIN)` 增加索引。
- 为 `security_lots(buy_transaction_id)` 增加索引。

### 2. 数据一致性校验
- 增加定期校验：用 `security_transactions` + `security_lot_sales` 重新计算 `security_lots.remaining_quantity`，发现不一致时告警或修复。
- 可写成后台任务或启动时检查。

### 3. 迁移策略
- 当前 V001/V002 尚未稳定发布，后续修改 schema 时需要补充新的迁移文件（V004、V005…），或在发布前统一整理 V002。  
- 由于当前采用 `CREATE TABLE IF NOT EXISTS`，在正式发布后新增字段/表需要单独的迁移脚本，避免覆盖用户数据。
