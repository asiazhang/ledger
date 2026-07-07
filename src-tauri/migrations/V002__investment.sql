-- V002 投资相关 schema：股票/基金/债券/ETF 等金融工具、证券交易流水、持仓批次
-- 注意：transactions.kind 的 CHECK 约束（含 buy/sell/dividend/split）已在 V001 中定义。

-- 1. instruments（金融工具字典表）
--    - 统一维护股票、基金、债券、ETF 等金融工具的基础信息。
--    - 交易、持仓批次通过 instrument_id 与它关联，避免重复录入名称和币种。
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
--    - 通过 instrument_id 关联 instruments，避免 symbol / instrument_type 重复录入和潜在不一致。
--    - 现金部分仍由 transactions 表表达，账户余额计算无需额外 JOIN。
--    - 分红/拆股等无资金变动时，transactions.amount_cents 为 0。
CREATE TABLE IF NOT EXISTS security_transactions (
    transaction_id   TEXT PRIMARY KEY REFERENCES transactions(id) ON DELETE CASCADE,  -- 关联交易 ID
    instrument_id    TEXT NOT NULL REFERENCES instruments(id),  -- 关联金融工具，通过 instruments 取 symbol / type / currency
    action           TEXT NOT NULL CHECK(action IN ('buy','sell','dividend','split')),  -- 交易动作：买入/卖出/分红/拆股
    quantity         REAL,                                -- 数量变化（拆股/送股/分红可用 NULL）
    price_cents      INTEGER,                             -- 成交单价（分），分红/拆股可为 NULL
    fee_cents        INTEGER NOT NULL DEFAULT 0           -- 手续费/佣金（分）
);

-- 3. security_lots（持仓批次表）
--    - 每笔买入交易产生一个 lot，记录独立的成本 basis。
--    - 支持 FIFO / LIFO / 平均成本 / 指定 lot 等卖出匹配规则。
--    - 卖出时通过 security_lot_sales 记录匹配的批次及已实现盈亏，并扣减 remaining_quantity。
--    - 拆股/送股等公司行为需要应用层调整所有相关 lot 的 quantity 和 cost_per_unit_cents。
CREATE TABLE IF NOT EXISTS security_lots (
    id                    TEXT PRIMARY KEY,  -- 批次全局唯一 ID（UUID v7）
    account_id            TEXT NOT NULL REFERENCES accounts(id),  -- 关联账户 ID
    instrument_id         TEXT NOT NULL REFERENCES instruments(id),  -- 关联金融工具
    buy_transaction_id    TEXT NOT NULL REFERENCES security_transactions(transaction_id) ON DELETE CASCADE,  -- 关联买入交易
    initial_quantity      REAL NOT NULL,                     -- 买入数量
    remaining_quantity    REAL NOT NULL,                     -- 剩余数量（卖出后扣减）
    cost_per_unit_cents   INTEGER NOT NULL,                  -- 单位成本（分），已含买入手续费摊薄
    currency_code         TEXT NOT NULL,                     -- 成本币种
    created_at            TEXT NOT NULL,                     -- 创建时间
    updated_at            TEXT NOT NULL,                     -- 最后更新时间
    version               INTEGER NOT NULL DEFAULT 1,          -- 版本计数
    device_id             TEXT NOT NULL,                       -- 创建设备/最后修改设备标识
    UNIQUE(account_id, instrument_id, buy_transaction_id)
);

-- 4. security_lot_sales（批次卖出匹配表）
--    - 记录一笔卖出交易匹配了哪些 lot、各卖出多少、对应的已实现盈亏。
--    - 是 realized_pnl 的审计来源，也是从 lot 重新计算持仓的依据。
CREATE TABLE IF NOT EXISTS security_lot_sales (
    id                  TEXT PRIMARY KEY,  -- 匹配记录全局唯一 ID（UUID v7）
    sell_transaction_id TEXT NOT NULL REFERENCES security_transactions(transaction_id) ON DELETE CASCADE,  -- 关联卖出交易
    lot_id              TEXT NOT NULL REFERENCES security_lots(id) ON DELETE CASCADE,  -- 关联被卖出的批次
    quantity            REAL NOT NULL,                     -- 卖出的该批次数量
    cost_per_unit_cents INTEGER NOT NULL,                  -- 卖出时该批次单位成本（分）
    realized_pnl_cents  INTEGER NOT NULL,                  -- 该匹配项已实现盈亏（分），已扣除卖出手续费
    currency_code       TEXT NOT NULL,                     -- 币种
    created_at          TEXT NOT NULL                      -- 创建时间
);

-- 5. market_prices（市场价格表）
--    - 按 instrument + 日期记录收盘价/最新价，用于计算持仓市值和未实现盈亏。
CREATE TABLE IF NOT EXISTS market_prices (
    id              TEXT PRIMARY KEY,  -- 价格记录全局唯一 ID（UUID v7）
    instrument_id   TEXT NOT NULL REFERENCES instruments(id),  -- 关联金融工具
    price_cents     INTEGER NOT NULL,        -- 最新/收盘价（分）
    currency_code   TEXT NOT NULL,           -- 报价币种
    priced_at       TEXT NOT NULL,           -- 日期或时间戳，ISO 8601 日期格式
    source          TEXT,                    -- 数据来源（如 yahoo、manual）
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    version         INTEGER NOT NULL DEFAULT 1,
    device_id       TEXT NOT NULL,
    UNIQUE(instrument_id, priced_at)
);

-- 6. v_holdings（当前持仓视图）
--    - 由 security_lots 实时聚合，不作为主数据存储，避免与交易流水不一致。
--    - 关联最新 market_prices 与 exchange_rates，输出账户本位币市值和未实现盈亏。
--    - 当无法取到行情或汇率时，market_value_cents / unrealized_pnl_cents 为 NULL。
DROP VIEW IF EXISTS v_holdings;
CREATE VIEW IF NOT EXISTS v_holdings AS
SELECT
    h.id,
    h.account_id,
    h.instrument_id,
    h.quantity,
    h.cost_basis_cents,
    h.currency_code AS cost_currency_code,
    p.price_cents AS latest_price_cents,
    p.currency_code AS latest_price_currency_code,
    CASE
        WHEN p.price_cents IS NULL THEN NULL
        WHEN p.currency_code = a.currency_code THEN CAST(ROUND(h.quantity * p.price_cents) AS INTEGER)
        WHEN er.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents * er.rate) AS INTEGER)
        ELSE NULL
    END AS market_value_cents,
    CASE
        WHEN p.price_cents IS NULL THEN NULL
        WHEN p.currency_code = a.currency_code THEN CAST(ROUND(h.quantity * p.price_cents) AS INTEGER) - h.cost_basis_cents
        WHEN er.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents * er.rate) AS INTEGER) - h.cost_basis_cents
        ELSE NULL
    END AS unrealized_pnl_cents,
    h.updated_at
FROM (
    SELECT
        account_id || '-' || instrument_id AS id,
        account_id,
        instrument_id,
        SUM(remaining_quantity) AS quantity,
        CAST(SUM(remaining_quantity * cost_per_unit_cents) AS INTEGER) AS cost_basis_cents,
        currency_code,
        MAX(updated_at) AS updated_at
    FROM security_lots
    WHERE remaining_quantity > 0
    GROUP BY account_id, instrument_id, currency_code
) h
LEFT JOIN accounts a ON a.id = h.account_id
LEFT JOIN (
    SELECT mp1.instrument_id, mp1.price_cents, mp1.currency_code
    FROM market_prices mp1
    WHERE mp1.priced_at = (
        SELECT MAX(mp2.priced_at)
        FROM market_prices mp2
        WHERE mp2.instrument_id = mp1.instrument_id
    )
) p ON p.instrument_id = h.instrument_id
LEFT JOIN (
    SELECT er1.base_code, er1.quote_code, er1.rate
    FROM exchange_rates er1
    WHERE er1.priced_at = (
        SELECT MAX(er2.priced_at)
        FROM exchange_rates er2
        WHERE er2.base_code = er1.base_code AND er2.quote_code = er1.quote_code
    )
) er ON er.base_code = p.currency_code AND er.quote_code = a.currency_code;

CREATE INDEX IF NOT EXISTS idx_security_lots_account_instrument ON security_lots(account_id, instrument_id);
CREATE INDEX IF NOT EXISTS idx_security_lots_buy_transaction ON security_lots(buy_transaction_id);
CREATE INDEX IF NOT EXISTS idx_security_lots_sync ON security_lots(updated_at, device_id);
CREATE INDEX IF NOT EXISTS idx_security_lot_sales_lot ON security_lot_sales(lot_id);
CREATE INDEX IF NOT EXISTS idx_security_transactions_instrument ON security_transactions(instrument_id);
CREATE INDEX IF NOT EXISTS idx_market_prices_instrument ON market_prices(instrument_id);
CREATE INDEX IF NOT EXISTS idx_market_prices_lookup ON market_prices(instrument_id, priced_at);
