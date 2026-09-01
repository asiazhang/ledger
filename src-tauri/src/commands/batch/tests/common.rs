//! 批量写入测试共享脚手架：仅限本测试目录内各子模块使用（跨测试模块合并不在此列，见 #250）。

use rusqlite::{Connection, params};

use crate::db::{init_db, open_in_memory};
use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;

pub(super) fn setup() -> Connection {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    conn
}

pub(super) fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name, kind, currency],
    ).unwrap();
}

pub(super) fn make_input(
    account_id: &str,
    kind: TransactionKind,
    amount: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}
