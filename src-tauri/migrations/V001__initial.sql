-- V001 初始 schema：面向多设备同步的离线优先设计
--
-- 设计说明：
-- 1. currencies：币种表，ISO 4217 代码作主键，金额按「分」整数存储。
-- 2. accounts / categories / transactions / budgets：主键均为 UUID v7（TEXT），全局唯一。
--    所有主表均携带 device_id / updated_at / version / is_deleted 同步字段，采用软删除 + LWW 冲突解决。
-- 3. transactions：交易表，核心表，所有金额以「分」为单位。
-- 4. budgets：预算表，按支出分类维度设置周期预算。
-- 5. exchange_rates：汇率表，多币种换算预留。
-- 投资相关表（instruments / security_transactions / holdings）见 V002__investment.sql。

CREATE TABLE IF NOT EXISTS currencies (
    code            TEXT PRIMARY KEY,           -- 货币代码（ISO 4217），如 CNY / USD / EUR
    name            TEXT NOT NULL,              -- 货币中文名称
    symbol          TEXT NOT NULL,              -- 展示符号，如 ¥ / $ / €
    decimal_places  INTEGER NOT NULL DEFAULT 2  -- 最小小数位，金额按分存储时用于格式化展示
);

CREATE TABLE IF NOT EXISTS accounts (
    id                    TEXT PRIMARY KEY,      -- 账户全局唯一 ID（UUID v7）
    name                  TEXT NOT NULL,                       -- 账户名称，如「现金」「招行储蓄卡」
    -- type 取值说明：
    -- cash：       手头现金（钱包、零钱），余额非负。
    -- bank：       银行借记/储蓄类账户（储蓄卡、工资卡、活期、定期、公积金等），余额非负。
    -- credit：     信用卡、花呗、白条等信用支付账户，余额可为负表示欠款。
    -- ewallet：    电子钱包（微信钱包、支付宝余额等第三方支付账户），余额非负。
    -- investment： 投资账户（股票、基金、债券、ETF 等证券资金账户），余额非负。
    -- debt：       负债账户（房贷、车贷、消费贷等），余额为负表示尚未偿还的欠款。
    -- receivable： 借出款/应收款账户，余额为正表示对方尚未归还的金额。
    -- other：      其他账户（押金、公司垫付、自定义账户等），作为兜底类型。
    type                  TEXT NOT NULL CHECK(type IN ('cash','bank','credit','ewallet','investment','debt','receivable','other')),
    currency_code         TEXT NOT NULL REFERENCES currencies(code),  -- 账户本位币代码，外键关联 currencies.code
    initial_balance_cents INTEGER NOT NULL DEFAULT 0,        -- 初始余额，以本位币「分」为单位的整数
    created_at            TEXT NOT NULL,                        -- 创建时间，UTC ISO 8601 格式
    updated_at            TEXT NOT NULL,                        -- 最后修改时间，UTC ISO 8601 格式（LWW 冲突解决）
    version               INTEGER NOT NULL DEFAULT 1,           -- 版本计数，每次修改 +1
    device_id             TEXT NOT NULL,                        -- 创建设备/最后修改设备标识
    is_deleted            INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1))  -- 软删除标志
);

CREATE TABLE IF NOT EXISTS categories (
    id         TEXT PRIMARY KEY,                  -- 分类全局唯一 ID（UUID v7）
    name       TEXT NOT NULL,                                      -- 分类名称，如「餐饮」「工资」
    kind       TEXT NOT NULL CHECK(kind IN ('income','expense')),   -- 分类类型：income（收入）或 expense（支出）
    parent_id  TEXT REFERENCES categories(id),                    -- 父分类 ID，NULL 表示顶级分类；指向同表 id
    icon       TEXT,                                               -- 图标名称（可选）
    color      TEXT,                                               -- 展示颜色（可选）
    created_at TEXT NOT NULL,                                       -- 创建时间，UTC ISO 8601 格式
    updated_at TEXT NOT NULL,                                       -- 最后修改时间，UTC ISO 8601 格式
    version    INTEGER NOT NULL DEFAULT 1,                          -- 版本计数
    device_id  TEXT NOT NULL,                                       -- 创建设备/最后修改设备标识
    is_deleted INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1))  -- 软删除标志
);

CREATE TABLE IF NOT EXISTS transactions (
    id                        TEXT PRIMARY KEY,                        -- 交易全局唯一 ID（UUID v7）
    -- kind 取值说明：
    -- income：  收入，增加 account_id 账户余额。
    -- expense： 支出，减少 account_id 账户余额。
    -- transfer：转账，从 account_id 转出，加到 to_account_id。
    -- refund：  退款，关联原 expense 交易（refund_of_transaction_id），退回原账户。
    -- buy：     买入证券/基金，减少账户现金，由 security_transactions 扩展记录持仓变化。
    -- sell：    卖出证券/基金，增加账户现金，由 security_transactions 扩展记录持仓变化。
    -- dividend：现金分红，增加账户现金，security_transactions 记录对应标的。
    -- split：   拆股/送股，不改变账户现金，仅通过 security_transactions 调整持仓数量。
    kind                      TEXT NOT NULL CHECK(kind IN ('income','expense','transfer','refund','buy','sell','dividend','split')),
    amount_cents              INTEGER NOT NULL,                                        -- 原始币种金额，以「分」为单位的整数
    currency_code             TEXT NOT NULL,                                           -- 原始币种代码，关联 currencies.code
    amount_native_cents       INTEGER NOT NULL,                                        -- 本位币金额（当前 1:1，预留多币种换算），以「分」为单位
    account_id                TEXT NOT NULL REFERENCES accounts(id),                 -- 关联账户 ID；支出/收入/转出账户
    to_account_id             TEXT REFERENCES accounts(id),                        -- 转入账户 ID，仅转账（transfer）时必填
    category_id               TEXT REFERENCES categories(id),                       -- 关联分类 ID，转账通常为空
    refund_of_transaction_id  TEXT REFERENCES transactions(id),                      -- 退款关联的原始支出交易 ID
    note                      TEXT,                                                    -- 交易备注（可选）
    date                      TEXT NOT NULL,                                           -- 交易日期，ISO 8601 日期格式（YYYY-MM-DD）
    created_at                TEXT NOT NULL,                                            -- 创建时间，UTC ISO 8601 格式
    updated_at                TEXT NOT NULL,                                            -- 最后修改时间，UTC ISO 8601 格式
    version                   INTEGER NOT NULL DEFAULT 1,                               -- 版本计数
    device_id                 TEXT NOT NULL,                                            -- 创建设备/最后修改设备标识
    is_deleted                INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1))   -- 软删除标志
);

CREATE TABLE IF NOT EXISTS budgets (
    id           TEXT PRIMARY KEY,  -- 预算全局唯一 ID（UUID v7）
    category_id  TEXT NOT NULL REFERENCES categories(id),  -- 关联支出分类 ID
    period       TEXT NOT NULL DEFAULT 'monthly',      -- 预算周期，如 monthly / yearly / weekly
    amount_cents INTEGER NOT NULL,                   -- 预算金额上限，以「分」为单位的整数
    start_date   TEXT NOT NULL,                       -- 预算开始日期，ISO 8601 日期格式（YYYY-MM-DD）
    created_at   TEXT NOT NULL,                         -- 创建时间，UTC ISO 8601 格式
    updated_at   TEXT NOT NULL,                         -- 最后修改时间，UTC ISO 8601 格式
    version      INTEGER NOT NULL DEFAULT 1,            -- 版本计数
    device_id    TEXT NOT NULL,                           -- 创建设备/最后修改设备标识
    is_deleted   INTEGER NOT NULL DEFAULT 0 CHECK(is_deleted IN (0, 1))  -- 软删除标志
);

CREATE TABLE IF NOT EXISTS exchange_rates (
    id         TEXT PRIMARY KEY,  -- 汇率记录全局唯一 ID（UUID v7）
    base_code  TEXT NOT NULL,                      -- 基础货币代码，如 USD
    quote_code TEXT NOT NULL,                      -- 报价货币代码，如 CNY
    rate       REAL NOT NULL,                      -- 汇率值，表示 1 base = ? quote
    updated_at TEXT NOT NULL,                       -- 更新时间，UTC ISO 8601 格式
    version    INTEGER NOT NULL DEFAULT 1,          -- 版本计数
    device_id  TEXT NOT NULL,                       -- 创建设备/最后修改设备标识
    UNIQUE(base_code, quote_code)
);

CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_transactions_account ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_transactions_category ON transactions(category_id);
CREATE INDEX IF NOT EXISTS idx_transactions_refund ON transactions(refund_of_transaction_id);
CREATE INDEX IF NOT EXISTS idx_transactions_sync ON transactions(updated_at, device_id);
CREATE INDEX IF NOT EXISTS idx_transactions_deleted ON transactions(is_deleted, updated_at);
CREATE INDEX IF NOT EXISTS idx_accounts_sync ON accounts(updated_at, device_id);
CREATE INDEX IF NOT EXISTS idx_categories_sync ON categories(updated_at, device_id);
CREATE INDEX IF NOT EXISTS idx_budgets_sync ON budgets(updated_at, device_id);
CREATE INDEX IF NOT EXISTS idx_categories_parent ON categories(parent_id);
