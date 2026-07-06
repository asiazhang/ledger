-- V002 投资相关 schema：股票/基金/债券/ETF 等金融工具、证券交易流水、持仓快照
-- 注意：transactions.kind 的 CHECK 约束（含 buy/sell/dividend/split）已在 V001 中定义。

-- 1. instruments（金融工具字典表）
--    - 统一维护股票、基金、债券、ETF 等金融工具的基础信息。
--    - 交易与持仓通过 (symbol, instrument_type) 与它关联，避免重复录入名称和币种。
--    - currency_code 表示该工具报价和交易的币种。
CREATE TABLE IF NOT EXISTS instruments (
    id              TEXT PRIMARY KEY,  -- 工具全局唯一 ID（UUID v7）
    symbol          TEXT NOT NULL,                      -- 代码，如 "600519.SH" / "NVDA" / "000001"
    instrument_type TEXT NOT NULL CHECK(instrument_type IN ('stock','fund','bond','etf','other')),  -- 金融工具类型
    name            TEXT,                                -- 名称（可选，如 "贵州茅台"）
    currency_code   TEXT NOT NULL,                     -- 报价币种
    created_at      TEXT NOT NULL,                     -- 创建时间
    updated_at      TEXT NOT NULL,                     -- 最后修改时间
    version         INTEGER NOT NULL DEFAULT 1,          -- 版本计数
    device_id       TEXT NOT NULL,                       -- 创建设备/最后修改设备标识
    UNIQUE(symbol, instrument_type)
);

-- 2. security_transactions（证券交易扩展表）
--    - 一对一关联 transactions，记录证券/基金的专用字段。
--    - 现金部分仍由 transactions 表表达，账户余额计算无需额外 JOIN。
--    - 分红/拆股等无资金变动时，transactions.amount_cents 为 0。
CREATE TABLE IF NOT EXISTS security_transactions (
    transaction_id   TEXT PRIMARY KEY REFERENCES transactions(id) ON DELETE CASCADE,  -- 关联交易 ID
    instrument_type  TEXT NOT NULL DEFAULT 'stock' CHECK(instrument_type IN ('stock','fund','bond','etf','other')),  -- 金融工具类型：股票/基金/债券/ETF/其他
    symbol           TEXT NOT NULL,                       -- 代码，如 "600519.SH" / "NVDA" / "000001"
    action           TEXT NOT NULL CHECK(action IN ('buy','sell','dividend','split')),  -- 交易动作：买入/卖出/分红/拆股
    quantity         REAL,                                -- 数量变化（拆股/送股/分红可用 NULL）
    price_cents      INTEGER,                             -- 成交单价（分），分红/拆股可为 NULL
    fee_cents        INTEGER NOT NULL DEFAULT 0           -- 手续费/佣金（分）
);

-- 3. holdings（持仓表）
--    - 记录每个账户下当前持有的金融工具快照，用于离线计算持仓市值和盈亏。
--    - 数量、总成本、已实现盈亏由证券交易流水聚合维护（也可在交易写入时增量更新）。
--    - 未实现盈亏需结合 market_prices（后续表）最新价格计算。
CREATE TABLE IF NOT EXISTS holdings (
    id                    TEXT PRIMARY KEY,  -- 持仓全局唯一 ID（UUID v7）
    account_id            TEXT NOT NULL REFERENCES accounts(id),  -- 关联账户 ID
    instrument_type       TEXT NOT NULL CHECK(instrument_type IN ('stock','fund','bond','etf','other')),  -- 金融工具类型
    symbol                TEXT NOT NULL,                     -- 代码，如 "600519.SH" / "NVDA" / "000001"
    name                  TEXT,                              -- 名称（可选，如 "贵州茅台"）
    quantity              REAL NOT NULL DEFAULT 0,           -- 当前持有数量
    cost_basis_cents      INTEGER NOT NULL DEFAULT 0,        -- 总成本（分），含买入时手续费
    realized_pnl_cents    INTEGER NOT NULL DEFAULT 0,          -- 已实现盈亏（分）
    currency_code         TEXT NOT NULL,                     -- 持仓币种
    created_at            TEXT NOT NULL,                     -- 创建时间
    updated_at            TEXT NOT NULL,                     -- 最后更新时间
    version               INTEGER NOT NULL DEFAULT 1,          -- 版本计数
    device_id             TEXT NOT NULL,                       -- 创建设备/最后修改设备标识
    UNIQUE(account_id, instrument_type, symbol)
);

CREATE INDEX IF NOT EXISTS idx_holdings_account ON holdings(account_id);
CREATE INDEX IF NOT EXISTS idx_holdings_sync ON holdings(updated_at, device_id);
