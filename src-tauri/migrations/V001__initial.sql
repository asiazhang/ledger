-- V001 初始 schema：从 init_db 原样迁出
-- 使用 IF NOT EXISTS 保证老用户库平滑过渡（已有表为 no-op）

CREATE TABLE IF NOT EXISTS currencies (
    code            TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    decimal_places  INTEGER NOT NULL DEFAULT 2
);

CREATE TABLE IF NOT EXISTS accounts (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    name                  TEXT NOT NULL,
    type                  TEXT NOT NULL,
    currency_code         TEXT NOT NULL,
    initial_balance_cents INTEGER NOT NULL DEFAULT 0,
    created_at            TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS categories (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK(kind IN ('income','expense')),
    parent_id  INTEGER,
    icon       TEXT,
    color      TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS transactions (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    kind                 TEXT NOT NULL CHECK(kind IN ('income','expense','transfer')),
    amount_cents         INTEGER NOT NULL,
    currency_code        TEXT NOT NULL,
    amount_native_cents  INTEGER NOT NULL,
    account_id           INTEGER NOT NULL,
    to_account_id        INTEGER,
    category_id          INTEGER,
    note                 TEXT,
    date                 TEXT NOT NULL,
    created_at           TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS budgets (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    category_id  INTEGER NOT NULL,
    period       TEXT NOT NULL DEFAULT 'monthly',
    amount_cents INTEGER NOT NULL,
    start_date   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS exchange_rates (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    base_code  TEXT NOT NULL,
    quote_code TEXT NOT NULL,
    rate       REAL NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_transactions_date ON transactions(date);
CREATE INDEX IF NOT EXISTS idx_transactions_account ON transactions(account_id);
CREATE INDEX IF NOT EXISTS idx_transactions_category ON transactions(category_id);
