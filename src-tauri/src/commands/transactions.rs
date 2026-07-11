use tauri::State;

use crate::commands::fx::convert_to_native;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Transaction, TransactionInput};

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
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(Transaction {
            id: r.get(0)?,
            kind: r.get(1)?,
            amount_cents: r.get(2)?,
            currency_code: r.get(3)?,
            amount_native_cents: r.get(4)?,
            account_id: r.get(5)?,
            to_account_id: r.get(6)?,
            category_id: r.get(7)?,
            refund_of_transaction_id: r.get(8)?,
            note: r.get(9)?,
            date: r.get(10)?,
            created_at: r.get(11)?,
            updated_at: r.get(12)?,
            version: r.get(13)?,
            device_id: r.get(14)?,
            is_deleted: r.get::<_, i64>(15)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_transaction(db: State<'_, DbState>, input: TransactionInput) -> Result<String> {
    if input.kind == "transfer" && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;

    if input.kind == "buy" {
        return crate::commands::investment::create_buy_transaction(&conn, input);
    }

    if input.kind == "sell" {
        return crate::commands::investment::create_sell_transaction(&conn, input);
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

    let native = convert_to_native(&conn, input.amount_cents, &currency_code, &account_id)?;
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
