-- V002 投资相关 schema：股票/基金/债券/ETF 等金融工具、证券交易流水、持仓批次
-- 注意：transactions.kind 的 CHECK 约束（含 buy/sell/dividend/split）已在 V001 中定义。
--
-- 【就地修改注记】本文件已被就地修改（两级 BREAKING 标记之一，另一级见
-- CHANGELOG「Unreleased」BREAKING 条目）：全部价格语义列（security_transactions.price_cents
-- / security_lots.cost_per_unit_cents / security_lot_sales.cost_per_unit_cents
-- / market_prices.price_cents）刻度由「分」重定义为万分之一元（0.0001 元，
-- 基金净值 4 位小数保真，ADR-0038），v_holdings 视图表达式同步换算
-- （金额分 = 数量 × 单价 ÷ 100）。另 market_prices 增 nav_date 列（净值日期，
-- 场外基金现价 = 最新公布单位净值时携带，兼任净值同步水位，issue #301），
-- v_holdings 增 latest_nav_date 列透传（持仓可见现价对应哪天的净值，issue #303）。
-- 刻度重定义列名保留、无 DDL 结构变化；nav_date / latest_nav_date 为结构增列，
-- 就地修改只影响全新安装；存量库不在兼容范围（裁定见 CHANGELOG BREAKING 条目
-- 与 ADR-0038 决策 5），不提供一次性处置工具。

-- 1. instruments（金融工具字典表）
--    - 统一维护股票、基金、债券、ETF 等金融工具的基础信息。
--    - 交易、持仓批次通过 instrument_id 与它关联，避免重复录入名称和币种。
--    - currency_code 表示该工具报价和交易的币种。
--    - market 表示工具所属市场：sh（上交所）/ sz（深交所）/ hk（港交所）/ unknown（其他，默认）。
CREATE TABLE IF NOT EXISTS instruments (
    id              TEXT PRIMARY KEY,  -- 工具全局唯一 ID（UUID v7）
    symbol          TEXT NOT NULL,                      -- 代码，如 "600519.SH" / "NVDA" / "000001"
    instrument_type TEXT NOT NULL CHECK(instrument_type IN ('stock','fund','bond','etf','other')),  -- 金融工具类型
    name            TEXT,                                -- 名称（可选，如 "贵州茅台"）
    currency_code   TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 报价币种
    market          TEXT NOT NULL DEFAULT 'unknown' CHECK(market IN ('sh','sz','hk','unknown')),  -- 所属市场
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
    instrument_id    TEXT NOT NULL REFERENCES instruments(id) ON DELETE RESTRICT,  -- 关联金融工具，通过 instruments 取 symbol / type / currency
    action           TEXT NOT NULL CHECK(action IN ('buy','sell','dividend','split')),  -- 交易动作：买入/卖出/分红/拆股
    quantity         REAL,                                -- 数量变化（拆股/送股/分红可用 NULL）
    price_cents      INTEGER,                             -- 成交单价（万分之一元，刻度见文件头就地修改注记），分红/拆股可为 NULL
    fee_cents        INTEGER NOT NULL DEFAULT 0           -- 手续费/佣金（分，金额列仍是整数分）
);

-- 3. security_lots（持仓批次表）
--    - 每笔买入交易产生一个 lot，记录独立的成本 basis。
--    - 支持 FIFO / LIFO / 平均成本 / 指定 lot 等卖出匹配规则。
--    - 卖出时通过 security_lot_sales 记录匹配的批次及已实现盈亏，并扣减 remaining_quantity。
--    - 拆股/送股等公司行为需要应用层调整所有相关 lot 的 quantity 和 cost_per_unit_cents。
CREATE TABLE IF NOT EXISTS security_lots (
    id                    TEXT PRIMARY KEY,  -- 批次全局唯一 ID（UUID v7）
    account_id            TEXT NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,  -- 关联账户 ID
    instrument_id         TEXT NOT NULL REFERENCES instruments(id) ON DELETE RESTRICT,  -- 关联金融工具
    buy_transaction_id    TEXT NOT NULL REFERENCES security_transactions(transaction_id) ON DELETE CASCADE,  -- 关联买入交易
    initial_quantity      REAL NOT NULL,                     -- 买入数量
    remaining_quantity    REAL NOT NULL,                     -- 剩余数量（卖出后扣减）
    cost_per_unit_cents   INTEGER NOT NULL,                  -- 单位成本（万分之一元，刻度见文件头就地修改注记），已含买入手续费摊薄
    currency_code         TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 成本币种
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
    cost_per_unit_cents INTEGER NOT NULL,                  -- 卖出时该批次单位成本（万分之一元，刻度见文件头就地修改注记）
    realized_pnl_cents  INTEGER NOT NULL,                  -- 该匹配项已实现盈亏（分），已扣除卖出手续费
    currency_code       TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 币种
    created_at          TEXT NOT NULL                      -- 创建时间
);

-- 5. market_prices（市场价格表）
--    - 每个 instrument 仅保留最新价格，用于计算持仓市值和未实现盈亏。
--    - priced_at 记录该价格对应的行情日期，updated_at 记录写入时间。
--    - nav_date 记录净值日期：场外基金现价 = 最新公布单位净值时携带（ADR-0038），
--      兼任净值同步水位；股票类现价无净值语义，恒为 NULL。
CREATE TABLE IF NOT EXISTS market_prices (
    id              TEXT PRIMARY KEY,  -- 价格记录全局唯一 ID（UUID v7）
    instrument_id   TEXT NOT NULL REFERENCES instruments(id) ON DELETE CASCADE,  -- 关联金融工具
    price_cents     INTEGER NOT NULL,        -- 最新价（万分之一元，刻度见文件头就地修改注记）
    currency_code   TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,  -- 报价币种
    priced_at       TEXT NOT NULL,           -- 行情日期，ISO 8601 日期格式
    nav_date        TEXT,                    -- 净值日期（场外基金现价 = 最新公布单位净值时携带，兼任净值同步水位，ADR-0038；股票恒 NULL）
    source          TEXT,                    -- 数据来源（如 yahoo、manual）
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    version         INTEGER NOT NULL DEFAULT 1,
    device_id       TEXT NOT NULL,
    UNIQUE(instrument_id)
);

-- 6. v_holdings（当前持仓视图）
--    - 由 security_lots 实时聚合，不作为主数据存储，避免与交易流水不一致。
--    - 关联最新 market_prices 与 exchange_rates，输出账户本位币市值和未实现盈亏。
--    - 金额列（成本/市值/盈亏）仍为整数分：数量 × 单价（万分之一元）÷ 100 = 分；
--      ÷ 100 换算已内联进表达式，与 trade/trend 的金额公式同口径（ADR-0038）。
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
    p.nav_date AS latest_nav_date,  -- 净值日期：基金现价（= 最新公布单位净值）携带，持仓可见现价对应哪天的净值（#303）；股票恒 NULL
    -- 有效汇率：优先正向 (价格币→账户币)，缺失时用反向 (账户币→价格币) 取倒数。
    -- 同币种时汇率视为 1（无需查表）。
    CASE
        WHEN p.price_cents IS NULL THEN NULL
        WHEN p.currency_code = a.currency_code THEN CAST(ROUND(h.quantity * p.price_cents / 100.0) AS INTEGER)
        WHEN er.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents * er.rate / 100.0) AS INTEGER)
        WHEN er_rev.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents / er_rev.rate / 100.0) AS INTEGER)
        ELSE NULL
    END AS market_value_cents,
    -- 未实现盈亏 = 账户币市值 − 账户币成本。市值经 er/er_rev（价格币→账户币）折算，
    -- 成本经 ec/ec_rev（lot 成本币→账户币）折算，两者同币后再相减；
    -- 任一折算缺失（行情或对应汇率不存在）时结果为 NULL。
    CASE
        WHEN p.price_cents IS NULL THEN NULL
        ELSE
            (CASE
                WHEN p.currency_code = a.currency_code THEN CAST(ROUND(h.quantity * p.price_cents / 100.0) AS INTEGER)
                WHEN er.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents * er.rate / 100.0) AS INTEGER)
                WHEN er_rev.rate IS NOT NULL THEN CAST(ROUND(h.quantity * p.price_cents / er_rev.rate / 100.0) AS INTEGER)
                ELSE NULL
            END)
            -
            (CASE
                WHEN h.currency_code = a.currency_code THEN h.cost_basis_cents
                WHEN ec.rate IS NOT NULL THEN CAST(ROUND(h.cost_basis_cents * ec.rate) AS INTEGER)
                WHEN ec_rev.rate IS NOT NULL THEN CAST(ROUND(h.cost_basis_cents / ec_rev.rate) AS INTEGER)
                ELSE NULL
            END)
    END AS unrealized_pnl_cents,
    h.updated_at
FROM (
    SELECT
        -- id 纳入 currency_code：GROUP BY 含 currency_code，若同账户同标的存在不同币种的 lot，
        -- 仅用 account_id-instrument_id 会生成重复 key。
        account_id || '-' || instrument_id || '-' || currency_code AS id,
        account_id,
        instrument_id,
        SUM(remaining_quantity) AS quantity,
        CAST(ROUND(SUM(remaining_quantity * cost_per_unit_cents) / 100.0) AS INTEGER) AS cost_basis_cents,
        currency_code,
        MAX(updated_at) AS updated_at
    FROM security_lots
    WHERE remaining_quantity > 0
      -- 排除软删除账户的持仓：在聚合前剔除已删账户的 lot，避免其持仓行进入视图。
      AND account_id IN (SELECT id FROM accounts WHERE is_deleted = 0)
    GROUP BY account_id, instrument_id, currency_code
) h
LEFT JOIN accounts a ON a.id = h.account_id
LEFT JOIN market_prices p ON p.instrument_id = h.instrument_id
-- er/er_rev：价格币→账户币的正向与反向（兜底）
LEFT JOIN exchange_rates er     ON er.base_code = p.currency_code     AND er.quote_code = a.currency_code
LEFT JOIN exchange_rates er_rev ON er_rev.base_code = a.currency_code AND er_rev.quote_code = p.currency_code
-- ec/ec_rev：lot 成本币→账户币的正向与反向（兜底）
LEFT JOIN exchange_rates ec     ON ec.base_code = h.currency_code     AND ec.quote_code = a.currency_code
LEFT JOIN exchange_rates ec_rev ON ec_rev.base_code = a.currency_code AND ec_rev.quote_code = h.currency_code;

-- v_holdings 聚合子查询按 (account_id, instrument_id, currency_code) GROUP BY 且 WHERE remaining_quantity > 0，
-- 故用 partial covering index：前三列对齐 GROUP BY 提供有序扫描免排序，后三列覆盖
-- SUM(remaining_quantity) / SUM(remaining_quantity * cost_per_unit_cents) / MAX(updated_at) 免回表。
-- account_id + instrument_id 查询已由 UNIQUE(account_id, instrument_id, buy_transaction_id) 自动索引覆盖，无需单独索引。
CREATE INDEX IF NOT EXISTS idx_security_lots_active_covering
    ON security_lots(account_id, instrument_id, currency_code, remaining_quantity, cost_per_unit_cents, updated_at)
    WHERE remaining_quantity > 0;
CREATE INDEX IF NOT EXISTS idx_security_lots_buy_transaction ON security_lots(buy_transaction_id);
CREATE INDEX IF NOT EXISTS idx_security_lots_sync ON security_lots(updated_at, device_id);
CREATE INDEX IF NOT EXISTS idx_security_lot_sales_lot ON security_lot_sales(lot_id);
CREATE INDEX IF NOT EXISTS idx_security_transactions_instrument ON security_transactions(instrument_id);
CREATE INDEX IF NOT EXISTS idx_market_prices_instrument ON market_prices(instrument_id);
