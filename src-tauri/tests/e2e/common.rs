use std::path::Path;

use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::db::{open_connection, open_connection_with_passphrase};
use ledger_infra::error::AppError;
use ledger_transaction::{Transaction, TransactionInput};

use crate::step_inputs::expense_input;
use crate::world::LedgerWorld;

pub fn assert_last_error_contains(world: &LedgerWorld, needle: &str) {
    match &world.last_error {
        Some(msg) => assert!(
            msg.contains(needle),
            "错误消息不匹配: 期望包含 '{needle}', 实际 '{msg}'"
        ),
        None => panic!("预期错误但未发生"),
    }
}

pub fn capture_expected_error<T>(world: &mut LedgerWorld, result: Result<T, AppError>) {
    world.last_error = match result {
        Err(AppError::Coded { message, .. }) => Some(message),
        _ => Some("预期失败但成功了".into()),
    };
}

pub fn count_transactions(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

pub fn count_transactions_in_file(db: &Path, passphrase: Option<&str>) -> i64 {
    let conn = match passphrase {
        Some(p) => open_connection_with_passphrase(db, p).unwrap(),
        None => open_connection(db).unwrap(),
    };
    count_transactions(&conn)
}

pub fn seed_account_with_expenses(
    conn: &Connection,
    account: &str,
    note_prefix: &str,
    count: usize,
    amount_base: i64,
    date: &str,
) -> String {
    let id = ledger_accounts::create_account(
        conn,
        ledger_accounts::AccountInput {
            name: account.into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(0),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .unwrap();
    for i in 0..count {
        let input = TransactionInput {
            note: Some(format!("{note_prefix} {i}")),
            ..expense_input(amount_base + i as i64, &id, date)
        };
        ledger_transaction::create_transaction_internal(conn, input).unwrap();
    }
    id
}

pub fn instrument_id_by_symbol(conn: &Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT id FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .expect("标的不存在，先铺垫 Given 存在标的")
}

pub fn query_all_transactions(conn: &Connection) -> Vec<Transaction> {
    let mut stmt = conn
        .prepare(
            "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
             to_account_id,funding_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
             version,device_id,is_deleted,merchant_id,policy_id,fx_rate_used,fx_rate_source \
             FROM transactions WHERE is_deleted=0 ORDER BY date DESC, created_at DESC, id DESC",
        )
        .unwrap();
    stmt.query_map([], |r| {
        Ok(Transaction {
            fundings: Vec::new(),
            id: r.get(0)?,
            kind: r.get(1)?,
            amount_cents: r.get(2)?,
            currency_code: r.get(3)?,
            amount_native_cents: r.get(4)?,
            account_id: r.get(5)?,
            to_account_id: r.get(6)?,
            funding_account_id: r.get(7)?,
            category_id: r.get(8)?,
            refund_of_transaction_id: r.get(9)?,
            source_order_no: None,
            note: r.get(10)?,
            date: r.get(11)?,
            created_at: r.get(12)?,
            updated_at: r.get(13)?,
            version: r.get(14)?,
            device_id: r.get(15)?,
            is_deleted: r.get::<_, i64>(16)? != 0,
            merchant_id: r.get(17)?,
            policy_id: r.get(18)?,
            fx_rate_used: r.get(19)?,
            fx_rate_source: r.get(20)?,
            source: None,
            convert: None,
            purchases: Vec::new(),
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub fn query_accounts_by_name(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM accounts WHERE is_deleted=0 AND is_hidden=0 ORDER BY created_at")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}
