-- Scheduled Transactions: 核心 + 扩展表设计 (ADR-0003)
-- 包含三类定时/定期交易：分期计划、订阅、定时转账

-- 核心计划表
CREATE TABLE IF NOT EXISTS scheduled_transactions (
    id              TEXT PRIMARY KEY,
    kind            TEXT NOT NULL CHECK (kind IN ('installment', 'subscription', 'scheduled_transfer')),
    status          TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'paused', 'cancelled', 'completed')),
    account_id      TEXT NOT NULL REFERENCES accounts(id),
    category_id     TEXT REFERENCES categories(id),
    amount_cents    INTEGER NOT NULL CHECK (amount_cents > 0),
    currency_code   TEXT NOT NULL REFERENCES currencies(code),
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
    scheduled_transaction_id TEXT NOT NULL REFERENCES scheduled_transactions(id),
    scheduled_date          TEXT NOT NULL,
    status                  TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'processing', 'completed', 'failed', 'cancelled')),
    transaction_id          TEXT REFERENCES transactions(id),
    amount_cents            INTEGER NOT NULL CHECK (amount_cents > 0),
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    version                 INTEGER NOT NULL DEFAULT 1,
    device_id               TEXT NOT NULL,
    is_deleted              INTEGER NOT NULL DEFAULT 0
);

-- 分期计划扩展表
CREATE TABLE IF NOT EXISTS installment_plans (
    scheduled_transaction_id TEXT PRIMARY KEY REFERENCES scheduled_transactions(id),
    counterparty            TEXT,
    total_amount_cents      INTEGER NOT NULL CHECK (total_amount_cents > 0),
    total_occurrences       INTEGER NOT NULL CHECK (total_occurrences >= 1)
);

-- 订阅扩展表
CREATE TABLE IF NOT EXISTS subscription_plans (
    scheduled_transaction_id TEXT PRIMARY KEY REFERENCES scheduled_transactions(id),
    counterparty            TEXT
);

-- 定时转账扩展表
CREATE TABLE IF NOT EXISTS scheduled_transfer_plans (
    scheduled_transaction_id TEXT PRIMARY KEY REFERENCES scheduled_transactions(id),
    to_account_id           TEXT NOT NULL REFERENCES accounts(id),
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
