//! Writer 接缝测试共享脚手架：仅限本测试目录（`writer::tests`）内部使用
//! （跨测试模块合并不在此列，见 #250）。

use rusqlite::{Connection, params};

use crate::transaction::amount::TransactionKind;
use crate::transaction::writer::{Input, insert_row, normalize};

pub(super) fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

pub(super) fn insert_account(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id) \
         VALUES (?1,?1,'cash',?2,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, currency],
    )
    .unwrap();
}

pub(super) fn insert_category(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO categories (id,name,kind,created_at,updated_at,version,device_id) \
         VALUES (?1,?1,'expense','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id],
    )
    .unwrap();
}

/// 通用入参构造器。
pub(super) fn input(kind: TransactionKind, amount_cents: i64, account_id: &str) -> Input {
    Input {
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        existing_merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-01".into(),
    }
}

/// 通过 writer 自身落一笔 expense（normalize + insert_row），作为退款来源。
pub(super) fn insert_source_expense(
    conn: &Connection,
    account_id: &str,
    category_id: Option<&str>,
) -> String {
    let norm = normalize(
        conn,
        &Input {
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            category_id: category_id.map(String::from),
            ..input(TransactionKind::Expense, 1000, account_id)
        },
    )
    .unwrap();
    insert_row(conn, &norm).unwrap()
}
