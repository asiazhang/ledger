use rusqlite::Connection;

use crate::error::Result;

/// 初始化数据库 schema 与默认种子数据。
pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
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
        "#,
    )?;
    seed_defaults(conn)?;
    Ok(())
}

/// 当库为空时写入默认币种与常用分类。
fn seed_defaults(conn: &Connection) -> Result<()> {
    let currency_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM currencies", [], |r| r.get(0))?;
    if currency_count == 0 {
        conn.execute(
            "INSERT INTO currencies (code,name,symbol,decimal_places) VALUES ('CNY','人民币','¥',2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO currencies (code,name,symbol,decimal_places) VALUES ('USD','美元','$',2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO currencies (code,name,symbol,decimal_places) VALUES ('EUR','欧元','€',2)",
            [],
        )?;
    }

    let cat_count: i64 = conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
    if cat_count == 0 {
        let now = now_iso();
        for name in [
            "餐饮",
            "交通",
            "购物",
            "住房",
            "娱乐",
            "医疗",
            "教育",
            "其他支出",
        ] {
            conn.execute(
                "INSERT INTO categories (name,kind,created_at) VALUES (?1,'expense',?2)",
                rusqlite::params![name, now],
            )?;
        }
        for name in ["工资", "奖金", "投资收益", "其他收入"] {
            conn.execute(
                "INSERT INTO categories (name,kind,created_at) VALUES (?1,'income',?2)",
                rusqlite::params![name, now],
            )?;
        }
    }

    Ok(())
}

/// 当前 UTC 时间 ISO 字符串。
pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}
