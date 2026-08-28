-- V010: 价格历史化（issue #136 / spec #135 / ADR-0019）
-- 新增 price_history（价格历史）与 fx_rate_history（汇率历史）两张表，
-- 承载「现价」（market_prices）之外的第二条价格承载线，供投资资产走势图消费。
-- 既有投资表（instruments / market_prices / v_holdings 等）已随 v0.3.0 发布冻结，
-- 本迁移只增不改；恢复旧版本备份后由本迁移自动补齐新表。
-- 周采样规则：每标的（/币种对）每周至多一条，取该周最后一个有报价交易日的价格；
-- 整周覆盖的幂等 upsert 由唯一约束保证，不产生重复。

-- 1. price_history（价格历史表）
--    - 周粒度价格序列：仅覆盖股票类持仓标的（口径同 InvestedInstrument）。
--    - trade_date 为周采样交易日（该周最后一个有报价交易日），ISO 8601 日期。
--    - price_cents 为整数分；currency_code 为报价币种（港股 HKD、沪深 CNY），
--      跨币种历史折算须配合 fx_rate_history 同期汇率，不用当前汇率近似历史。
--    - 已清仓标的的历史保留不删，供回看过去的组合市值。
--    - UNIQUE(instrument_id, trade_date) 即「每标的每周至多一条」的唯一约束语义，
--      同时充当按标的 + 采样日区间查询的索引（单标的走势查询路径）。
CREATE TABLE IF NOT EXISTS price_history (
    id             TEXT PRIMARY KEY,  -- 价格历史记录全局唯一 ID（UUID v7）
    instrument_id  TEXT NOT NULL REFERENCES instruments(id) ON DELETE CASCADE,  -- 关联金融工具，标的删除时历史级联删除
    trade_date     TEXT NOT NULL,     -- 周采样交易日，ISO 8601 日期（YYYY-MM-DD）
    price_cents    INTEGER NOT NULL,  -- 收盘价（分，整数分）
    currency_code  TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 报价币种
    source         TEXT,              -- 数据来源（如 eastmoney）
    created_at     TEXT NOT NULL,     -- 创建时间，UTC ISO 8601
    updated_at     TEXT NOT NULL,     -- 最后修改时间，UTC ISO 8601
    version        INTEGER NOT NULL DEFAULT 1,  -- 版本计数
    device_id      TEXT NOT NULL,     -- 创建设备/最后修改设备标识
    UNIQUE(instrument_id, trade_date) -- 每标的每周至多一条（周采样唯一约束语义）
);

-- 2. fx_rate_history（汇率历史表）
--    - 与 PriceHistory 同源同时段采集的周粒度汇率序列，用于把非默认币种的
--      历史市值折算到 DefaultCurrency；当期折算仍走 exchange_rates，二者并存分工。
--    - 规则与 PriceHistory 对齐：周采样、同周整周覆盖（幂等）、正反向兜底由查询层实现。
--    - UNIQUE(base_code, quote_code, trade_date) 即「币种对 × 周采样日唯一」。
CREATE TABLE IF NOT EXISTS fx_rate_history (
    id             TEXT PRIMARY KEY,  -- 汇率历史记录全局唯一 ID（UUID v7）
    base_code      TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 基础货币代码，如 HKD
    quote_code     TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 报价货币代码，如 CNY
    trade_date     TEXT NOT NULL,     -- 周采样交易日，ISO 8601 日期（YYYY-MM-DD）
    rate           REAL NOT NULL,     -- 汇率值，表示 1 base = ? quote（与 exchange_rates 口径一致）
    source         TEXT,              -- 数据来源（如 eastmoney）
    created_at     TEXT NOT NULL,     -- 创建时间，UTC ISO 8601
    updated_at     TEXT NOT NULL,     -- 最后修改时间，UTC ISO 8601
    version        INTEGER NOT NULL DEFAULT 1,  -- 版本计数
    device_id      TEXT NOT NULL,     -- 创建设备/最后修改设备标识
    UNIQUE(base_code, quote_code, trade_date) -- 币种对 × 周采样日唯一
);

-- 基础索引：走势查询按「标的 + 时间区间」取数路径。
-- price_history 的单标的路径已由 UNIQUE(instrument_id, trade_date) 自动索引覆盖；
-- 此处补组合走势需要的跨标的时间区间扫描（同区间内所有采样点）。
CREATE INDEX IF NOT EXISTS idx_price_history_trade_date ON price_history(trade_date);
CREATE INDEX IF NOT EXISTS idx_fx_rate_history_trade_date ON fx_rate_history(trade_date);
