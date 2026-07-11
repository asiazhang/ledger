use rusqlite::Connection;
use tauri::State;

use crate::commands::fx::convert_to_native;
use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{CreateTransactionResult, Transaction, TransactionInput};

fn insert_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    if input.kind == "transfer" && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }

    if input.kind == "buy" {
        return crate::commands::investment::create_buy_transaction(conn, input);
    }

    if input.kind == "sell" {
        return crate::commands::investment::create_sell_transaction(conn, input);
    }

    if input.amount_cents <= 0 {
        return Err(AppError::Invalid("金额必须大于 0".into()));
    }

    let (category_id, account_id, currency_code, refund_of_id) = if input.kind == "refund" {
        let ref_id = input
            .refund_of_transaction_id
            .ok_or_else(|| AppError::Invalid("退款必须关联原支出交易".into()))?;
        let (cat, acc, cur, okind): (Option<String>, String, String, String) = conn.query_row(
            "SELECT category_id, account_id, currency_code, kind \
             FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![ref_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if okind != "expense" {
            return Err(AppError::Invalid("退款只能关联支出交易".into()));
        }
        (cat, acc, cur, Some(ref_id))
    } else {
        (
            input.category_id,
            input.account_id,
            input.currency_code,
            None,
        )
    };

    let native = convert_to_native(conn, input.amount_cents, &currency_code, &account_id)?;
    let to_account_id = if input.kind == "transfer" {
        input.to_account_id
    } else {
        None
    };
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0)",
        rusqlite::params![
            id,
            input.kind,
            input.amount_cents,
            currency_code,
            native,
            account_id,
            to_account_id,
            category_id,
            refund_of_id,
            input.note,
            input.date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn list_transactions(db: State<'_, DbState>, limit: Option<i64>) -> Result<Vec<Transaction>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let base_sql = "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted \
         FROM transactions WHERE is_deleted=0 ORDER BY date DESC, created_at DESC";
    let sql = match limit {
        Some(n) => format!("{base_sql} LIMIT {n}"),
        None => String::from(base_sql),
    };
    query_all(&conn, &sql, [])
}

#[tauri::command]
pub fn create_transaction(db: State<'_, DbState>, input: TransactionInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    insert_transaction(&conn, input)
}

#[tauri::command]
pub fn create_transactions(
    db: State<'_, DbState>,
    inputs: Vec<TransactionInput>,
) -> Result<Vec<CreateTransactionResult>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute("BEGIN", [])?;
    let mut results = Vec::with_capacity(inputs.len());
    for input in inputs {
        match insert_transaction(&conn, input) {
            Ok(id) => results.push(CreateTransactionResult {
                success: true,
                id: Some(id),
                error: None,
            }),
            Err(AppError::Invalid(msg)) => results.push(CreateTransactionResult {
                success: false,
                id: None,
                error: Some(msg),
            }),
            Err(e) => {
                conn.execute("ROLLBACK", [])?;
                return Err(e);
            }
        }
    }
    conn.execute("COMMIT", [])?;
    Ok(results)
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;

    let is_buy: Option<bool> = conn
        .query_row(
            "SELECT kind='buy' FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )
        .ok();
    if is_buy == Some(true) {
        let sold: i64 = conn.query_row(
            "SELECT COUNT(*) FROM security_lots \
             WHERE buy_transaction_id=(SELECT transaction_id FROM security_transactions WHERE transaction_id=?1) \
             AND remaining_quantity < initial_quantity",
            rusqlite::params![id],
            |r| r.get(0),
        )?;
        if sold > 0 {
            return Err(AppError::Invalid("该买入交易已有部分卖出，无法删除".into()));
        }
        conn.execute(
            "DELETE FROM security_lots WHERE buy_transaction_id=(SELECT transaction_id FROM security_transactions WHERE transaction_id=?1)",
            rusqlite::params![id],
        )?;
        conn.execute(
            "DELETE FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![id],
        )?;
    }

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{init_db, open_in_memory};
    use rusqlite::params;

    fn setup() -> Connection {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        conn
    }

    fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![id, name, kind, currency],
        ).unwrap();
    }

    fn make_input(account_id: &str, kind: &str, amount: i64, date: &str) -> TransactionInput {
        TransactionInput {
            kind: kind.into(),
            amount_cents: amount,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: date.into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
        }
    }

    #[test]
    fn batch_creates_all_valid_transactions() {
        let conn = setup();
        insert_account(&conn, "acc-batch", "现金", "cash", "CNY");

        let inputs = vec![
            make_input("acc-batch", "income", 1000, "2026-01-01"),
            make_input("acc-batch", "expense", 500, "2026-01-02"),
            make_input("acc-batch", "income", 2000, "2026-01-03"),
        ];

        let results = inputs
            .into_iter()
            .map(|i| insert_transaction(&conn, i))
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(results.len(), 3);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn batch_rejects_transfer_without_to_account() {
        let conn = setup();
        insert_account(&conn, "acc-batch2", "现金", "cash", "CNY");

        let result = insert_transaction(
            &conn,
            make_input("acc-batch2", "transfer", 1000, "2026-01-01"),
        );
        match result {
            Err(AppError::Invalid(msg)) => assert!(msg.contains("目标账户")),
            _ => panic!("expected Invalid error"),
        }
    }

    #[test]
    fn batch_rejects_zero_amount() {
        let conn = setup();
        insert_account(&conn, "acc-batch2", "现金", "cash", "CNY");

        let bad = TransactionInput {
            amount_cents: 0,
            ..make_input("acc-batch2", "income", 100, "2026-01-01")
        };
        let result = insert_transaction(&conn, bad);
        match result {
            Err(AppError::Invalid(msg)) => assert!(msg.contains("大于 0")),
            _ => panic!("expected Invalid error"),
        }
    }
}
