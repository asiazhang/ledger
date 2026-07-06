-- V001 初始 schema：从 init_db 原样迁出
-- 使用 IF NOT EXISTS 保证老用户库平滑过渡（已有表为 no-op）
--
-- 表设计说明：
-- 1. currencies：币种表，ISO 4217 代码作主键，金额按「分」整数存储。
-- 2. accounts：账户表，余额不持久化，由交易实时聚合。
-- 3. categories：分类表，支持二级分类。
-- 4. transactions：交易表，核心表，所有金额以「分」为单位。
-- 5. budgets：预算表，按支出分类维度设置周期预算。
-- 6. exchange_rates：汇率表，多币种换算预留。
-- 投资相关表（instruments / security_transactions / holdings）见 V002__investment.sql。

CREATE TABLE IF NOT EXISTS currencies (
    code            TEXT PRIMARY KEY,           -- 货币代码（ISO 4217），如 CNY / USD / EUR
    name            TEXT NOT NULL,              -- 货币中文名称
    symbol          TEXT NOT NULL,              -- 展示符号，如 ¥ / $ / €
    decimal_places  INTEGER NOT NULL DEFAULT 2  -- 最小小数位，金额按分存储时用于格式化展示
);

CREATE TABLE IF NOT EXISTS accounts (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,  -- 账户唯一 ID
    name                  TEXT NOT NULL,                       -- 账户名称，如「现金」「招行储蓄卡」
    -- type 取值说明：
    -- cash：       手头现金（钱包、零钱），余额非负。
    -- bank：       银行借记类账户（储蓄卡、工资卡、活期账户），余额非负，不能透支。
    -- credit：     信用卡、花呗、白条等信用支付账户，余额可为负表示欠款。
    -- savings：    专门储蓄账户（定期、公积金、应急金），余额非负，通常不做日常消费。
    -- ewallet：    电子钱包（微信钱包、支付宝余额等第三方支付账户），余额非负。
    -- debt：       负债账户（房贷、车贷、消费贷等），余额为负表示尚未偿还的欠款。
    -- receivable： 债权账户（借出款项、应收款），余额为正表示对方尚未归还的金额。
    type                  TEXT NOT NULL CHECK(type IN ('cash','bank','credit','savings','ewallet','debt','receivable')),
    currency_code         TEXT NOT NULL REFERENCES currencies(code),  -- 账户本位币代码，外键关联 currencies.code
    initial_balance_cents INTEGER NOT NULL DEFAULT 0,        -- 初始余额，以本位币「分」为单位的整数
    created_at            TEXT NOT NULL                        -- 创建时间，UTC ISO 8601 格式
);

CREATE TABLE IF NOT EXISTS categories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,                  -- 分类唯一 ID
    name       TEXT NOT NULL,                                      -- 分类名称，如「餐饮」「工资」
    kind       TEXT NOT NULL CHECK(kind IN ('income','expense')),   -- 分类类型：income（收入）或 expense（支出）
    parent_id  INTEGER REFERENCES categories(id),                    -- 父分类 ID，NULL 表示顶级分类；指向同表 id
    icon       TEXT,                                               -- 图标名称（可选）
    color      TEXT,                                               -- 展示颜色（可选）
    created_at TEXT NOT NULL                                       -- 创建时间，UTC ISO 8601 格式
);

CREATE TABLE IF NOT EXISTS transactions (
    id                        INTEGER PRIMARY KEY AUTOINCREMENT,                        -- 交易唯一 ID
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
    account_id                INTEGER NOT NULL REFERENCES accounts(id),                 -- 关联账户 ID；支出/收入/转出账户
    to_account_id             INTEGER REFERENCES accounts(id),                        -- 转入账户 ID，仅转账（transfer）时必填
    category_id               INTEGER REFERENCES categories(id),                       -- 关联分类 ID，转账通常为空
    refund_of_transaction_id  INTEGER REFERENCES transactions(id),                      -- 退款关联的原始支出交易 ID
    note                      TEXT,                                                    -- 交易备注（可选）
    date                      TEXT NOT NULL,                                           -- 交易日期，ISO 8601 日期格式（YYYY-MM-DD）
    created_at                TEXT NOT NULL                                            -- 创建时间，UTC ISO 8601 格式
);

CREATE TABLE IF NOT EXISTS budgets (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,  -- 预算唯一 ID
    category_id  INTEGER NOT NULL REFERENCES categories(id),  -- 关联支出分类 ID
    period       TEXT NOT NULL DEFAULT 'monthly',      -- 预算周期，如 monthly / yearly / weekly
    amount_cents INTEGER NOT NULL,                   -- 预算金额上限，以「分」为单位的整数
    start_date   TEXT NOT NULL                       -- 预算开始日期，ISO 8601 日期格式（YYYY-MM-DD）
);

CREATE TABLE IF NOT EXISTS exchange_rates (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,  -- 汇率记录唯一 ID
    base_code  TEXT NOT NULL,                      -- 基础货币代码，如 USD
    quote_code TEXT NOT NULL,                      -- 报价货币代码，如 CNY
    rate       REAL NOT NULL,                      -- 汇率值，表示 1 base = ? quote
    updated_at TEXT NOT NULL                       -- 更新时间，UTC ISO 8601 格式
);

CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_transactions_account ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_transactions_category ON transactions(category_id);
CREATE INDEX IF NOT EXISTS idx_transactions_refund ON transactions(refund_of_transaction_id);
