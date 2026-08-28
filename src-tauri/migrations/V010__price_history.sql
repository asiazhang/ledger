-- V010: 价格历史化（issue #136 / spec #135 / ADR-0019）
-- 新增 price_history（价格历史）与 fx_rate_history（汇率历史）两张表，
-- 承载「现价」（market_prices）之外的第二条价格承载线，供投资资产走势图消费。
-- 既有投资表（instruments / market_prices / v_holdings 等）已随 v0.3.0 发布冻结，
-- 本迁移只增不改；恢复旧版本备份后由本迁移自动补齐新表。
-- 周采样规则：每标的（/币种对）每周至多一条，取该周最后一个有报价交易日的价格；
-- 「整周覆盖」的幂等 upsert 由周唯一约束保证（同周任一采样日写入都落在同一周键上），
-- 不产生重复。

-- 1. price_history（价格历史表）
--    - 周粒度价格序列：仅覆盖股票类持仓标的（口径同 InvestedInstrument）。
--    - trade_date 为周采样交易日（该周最后一个有报价交易日），ISO 8601 日期。
--    - price_cents 为整数分；currency_code 为报价币种（港股 HKD、沪深 CNY），
--      跨币种历史折算须配合 fx_rate_history 同期汇率，不用当前汇率近似历史。
--    - 已清仓标的的历史保留不删，供回看过去的组合市值（清仓不触发本表删除；
--      仅标的本身被删除时级联跟随，与 market_prices 同策略）。
--    - week_start 为 trade_date 所属 ISO 周的周一（STORED 生成列，周一为首日，
--      与 A股/港股交易日历的周一致），「每标的每周至多一条」由
--      UNIQUE(instrument_id, week_start) 在库层强制成立。
CREATE TABLE IF NOT EXISTS price_history (
    id             TEXT PRIMARY KEY,  -- 价格历史记录全局唯一 ID（UUID v7）
    instrument_id  TEXT NOT NULL REFERENCES instruments(id) ON DELETE CASCADE,  -- 关联金融工具，标的删除时历史级联删除
    trade_date     TEXT NOT NULL,     -- 周采样交易日，ISO 8601 日期（YYYY-MM-DD）
    week_start     TEXT NOT NULL GENERATED ALWAYS AS (date(trade_date, '-6 days', 'weekday 1')) STORED,  -- 所属 ISO 周的周一，由 trade_date 派生，周唯一键
    price_cents    INTEGER NOT NULL,  -- 收盘价（分，整数分）
    currency_code  TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 报价币种
    source         TEXT,              -- 数据来源（如 eastmoney）
    created_at     TEXT NOT NULL,     -- 创建时间，UTC ISO 8601
    updated_at     TEXT NOT NULL,     -- 最后修改时间，UTC ISO 8601
    version        INTEGER NOT NULL DEFAULT 1,  -- 版本计数
    device_id      TEXT NOT NULL,     -- 创建设备/最后修改设备标识
    UNIQUE(instrument_id, week_start) -- 每标的每周至多一条（ADR-0019 周采样决策）
);

-- 2. fx_rate_history（汇率历史表）
--    - 与 PriceHistory 同源同时段采集的周粒度汇率序列，用于把非默认币种的
--      历史市值折算到 DefaultCurrency；当期折算仍走 exchange_rates，二者并存分工。
--    - 规则与 PriceHistory 对齐：周采样、同周整周覆盖（幂等）、正反向兜底由查询层实现。
--    - 「币种对 × 周唯一」由 UNIQUE(base_code, quote_code, week_start) 强制成立。
CREATE TABLE IF NOT EXISTS fx_rate_history (
    id             TEXT PRIMARY KEY,  -- 汇率历史记录全局唯一 ID（UUID v7）
    base_code      TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 基础货币代码，如 HKD
    quote_code     TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 报价货币代码，如 CNY
    trade_date     TEXT NOT NULL,     -- 周采样交易日，ISO 8601 日期（YYYY-MM-DD）
    week_start     TEXT NOT NULL GENERATED ALWAYS AS (date(trade_date, '-6 days', 'weekday 1')) STORED,  -- 所属 ISO 周的周一，由 trade_date 派生，周唯一键
    rate           REAL NOT NULL,     -- 汇率值，表示 1 base = ? quote（与 exchange_rates 口径一致）
    source         TEXT,              -- 数据来源（如 eastmoney）
    created_at     TEXT NOT NULL,     -- 创建时间，UTC ISO 8601
    updated_at     TEXT NOT NULL,     -- 最后修改时间，UTC ISO 8601
    version        INTEGER NOT NULL DEFAULT 1,  -- 版本计数
    device_id      TEXT NOT NULL,     -- 创建设备/最后修改设备标识
    UNIQUE(base_code, quote_code, week_start) -- 币种对 × 周唯一
);

-- 基础索引：走势查询按「标的 + 时间区间」取数路径（供 T3 走势查询使用）。
-- 单标的按采样日区间的扫描路径由 idx_price_history_instrument_date 提供；
-- idx_*_trade_date 补跨标的/跨币种对的同时间区间扫描路径。
CREATE INDEX IF NOT EXISTS idx_price_history_instrument_date ON price_history(instrument_id, trade_date);
CREATE INDEX IF NOT EXISTS idx_price_history_trade_date ON price_history(trade_date);
CREATE INDEX IF NOT EXISTS idx_fx_rate_history_trade_date ON fx_rate_history(trade_date);
