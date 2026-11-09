-- Scheduled Transactions: 核心 + 扩展表设计 (ADR-0003)
-- 包含三类定时/定期交易：分期计划、订阅、定时转账
--
-- 【就地修改注记】本文件已被就地修改（两级 BREAKING 标记之一，另一级见
-- CHANGELOG「Unreleased」BREAKING 条目）：为 9 个引用列补全显式 ON DELETE——
-- 强依赖 RESTRICT / 溯源指针（可空分类、生成交易）SET NULL /
-- 期次与计划扩展表行 CASCADE。
-- 就地修改只影响全新安装；已执行过本迁移的存量库与旧备份恢复路径保持原
-- NO ACTION（当前无硬删路径、差异不可达、零行为差异），未来首个依赖新语义的
-- 功能发布时自带收敛迁移。

-- 核心计划表
CREATE TABLE IF NOT EXISTS scheduled_transactions (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL CHECK (kind IN ('installment', 'subscription', 'scheduled_transfer')),
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'cancelled', 'completed')),
    account_id      TEXT NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    category_id     TEXT REFERENCES categories(id) ON DELETE SET NULL,
    amount_cents    INTEGER NOT NULL CHECK (amount_cents > 0),
    currency_code   TEXT NOT NULL REFERENCES currencies(code) ON DELETE RESTRICT,
    recurrence_type     TEXT NOT NULL CHECK (recurrence_type IN ('daily', 'weekly', 'monthly', 'yearly')),
    recurrence_interval INTEGER NOT NULL DEFAULT 1 CHECK (recurrence_interval > 0),
    recurrence_day      INTEGER,
    start_date      TEXT NOT NULL,
    note            TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    version         INTEGER NOT NULL DEFAULT 1,
    device_id       TEXT NOT NULL,
    is_deleted      INTEGER NOT NULL DEFAULT 0
);

-- 统一期次表
CREATE TABLE IF NOT EXISTS scheduled_transaction_occurrences (
    id                      TEXT PRIMARY KEY,
    scheduled_transaction_id TEXT NOT NULL REFERENCES scheduled_transactions(id) ON DELETE CASCADE,
    scheduled_date          TEXT NOT NULL,
    status                  TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'cancelled')),
    transaction_id          TEXT REFERENCES transactions(id) ON DELETE SET NULL,
    amount_cents            INTEGER NOT NULL CHECK (amount_cents > 0),
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    version                 INTEGER NOT NULL DEFAULT 1,
    device_id               TEXT NOT NULL,
    is_deleted              INTEGER NOT NULL DEFAULT 0
);

-- 分期计划扩展表（issue #190 / ADR-0028）：counterparty 文本列改为 merchant_id 商户引用
-- （硬删置空，与 transactions.merchant_id 同语义；每期生成交易时复制到流水）
CREATE TABLE IF NOT EXISTS installment_plans (
    scheduled_transaction_id TEXT PRIMARY KEY REFERENCES scheduled_transactions(id) ON DELETE CASCADE,
    merchant_id             TEXT REFERENCES merchants(id) ON DELETE SET NULL,
    total_amount_cents      INTEGER NOT NULL CHECK (total_amount_cents > 0),
    total_occurrences       INTEGER NOT NULL CHECK (total_occurrences >= 1)
);

-- 订阅扩展表（issue #190 / ADR-0028）：同 installment_plans，counterparty → merchant_id
CREATE TABLE IF NOT EXISTS subscription_plans (
    scheduled_transaction_id TEXT PRIMARY KEY REFERENCES scheduled_transactions(id) ON DELETE CASCADE,
    merchant_id             TEXT REFERENCES merchants(id) ON DELETE SET NULL
);

-- 定时转账扩展表
CREATE TABLE IF NOT EXISTS scheduled_transfer_plans (
    scheduled_transaction_id TEXT PRIMARY KEY REFERENCES scheduled_transactions(id) ON DELETE CASCADE,
    to_account_id           TEXT NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    total_occurrences       INTEGER CHECK (total_occurrences IS NULL OR total_occurrences >= 1)
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_scheduled_transactions_account
    ON scheduled_transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_scheduled_transactions_kind_status
    ON scheduled_transactions(kind, status);
CREATE INDEX IF NOT EXISTS idx_scheduled_transactions_sync
    ON scheduled_transactions(updated_at, device_id);
CREATE INDEX IF NOT EXISTS idx_scheduled_occurrences_plan_date
    ON scheduled_transaction_occurrences(scheduled_transaction_id, scheduled_date);
CREATE INDEX IF NOT EXISTS idx_scheduled_occurrences_due
    ON scheduled_transaction_occurrences(scheduled_date, status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_scheduled_occurrences_txn
    ON scheduled_transaction_occurrences(transaction_id) WHERE transaction_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_scheduled_occurrences_sync
    ON scheduled_transaction_occurrences(updated_at, device_id);
