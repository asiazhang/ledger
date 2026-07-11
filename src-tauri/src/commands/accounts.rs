use rusqlite::Connection;
use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Account, AccountBalance, AccountInput};

#[tauri::command]
pub fn list_accounts(db: State<'_, DbState>) -> Result<Vec<Account>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
         FROM accounts WHERE is_deleted=0 ORDER BY created_at",
        [],
    )
}

#[tauri::command]
pub fn create_account(db: State<'_, DbState>, input: AccountInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![
            id,
            input.name,
            input.kind,
            input.currency_code,
            input.initial_balance_cents.unwrap_or(0),
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_account(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 计算账户当前余额 = 初始余额 + 收入 - 支出（转账从转出账户减，加到转入账户）。
fn account_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    let initial: i64 = conn.query_row(
        "SELECT initial_balance_cents FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let income: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='income' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let expense: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='expense' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_in: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE to_account_id=?1 AND kind='transfer' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_out: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='transfer' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let refund: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='refund' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    Ok(
        initial + income.unwrap_or(0) - expense.unwrap_or(0) + transfer_in.unwrap_or(0)
            - transfer_out.unwrap_or(0)
            + refund.unwrap_or(0),
    )
}

#[tauri::command]
pub fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let accounts = query_all(
        &conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
         FROM accounts WHERE is_deleted=0 ORDER BY created_at",
        [],
    )?;
    accounts
        .into_iter()
        .map(|a: Account| {
            Ok(AccountBalance {
                balance_cents: account_balance(&conn, &a.id)?,
                account: a,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::db::query::query_all;
    use crate::db::{device_id, new_uuid, now_iso};
    use crate::models::Account;

    fn setup() -> rusqlite::Connection {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn list_accounts(conn: &rusqlite::Connection) -> Vec<Account> {
        query_all(
            conn,
            "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
             FROM accounts WHERE is_deleted=0 ORDER BY created_at",
            [],
        )
        .unwrap()
    }

    fn insert_account(conn: &rusqlite::Connection, id: &str, name: &str, kind: &str, currency: &str, initial: i64) {
        let now = now_iso();
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
            rusqlite::params![id, name, kind, currency, initial, now, now, 1, device_id()],
        ).unwrap();
    }

    fn insert_tx(conn: &rusqlite::Connection, id: &str, kind: &str, amount: i64, account_id: &str, to_account_id: Option<&str>) {
        let now = now_iso();
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,'CNY',?3,?4,?5,NULL,NULL,NULL,'2026-01-15',?6,?7,?8,?9,0)",
            rusqlite::params![id, kind, amount, account_id, to_account_id, now, now, 1, device_id()],
        ).unwrap();
    }

    fn balance(conn: &rusqlite::Connection, account_id: &str) -> i64 {
        let initial: i64 = conn.query_row(
            "SELECT initial_balance_cents FROM accounts WHERE id=?1",
            rusqlite::params![account_id],
            |r| r.get(0),
        ).unwrap();
        let income: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
                 WHERE account_id=?1 AND kind='income' AND is_deleted=0",
                rusqlite::params![account_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let expense: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
                 WHERE account_id=?1 AND kind='expense' AND is_deleted=0",
                rusqlite::params![account_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let transfer_in: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
                 WHERE to_account_id=?1 AND kind='transfer' AND is_deleted=0",
                rusqlite::params![account_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let transfer_out: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
                 WHERE account_id=?1 AND kind='transfer' AND is_deleted=0",
                rusqlite::params![account_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let refund: i64 = conn
            .query_row(
                "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
                 WHERE account_id=?1 AND kind='refund' AND is_deleted=0",
                rusqlite::params![account_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        initial + income - expense + transfer_in - transfer_out + refund
    }

    #[test]
    fn list_accounts_empty_initially() {
        let conn = setup();
        let accounts = list_accounts(&conn);
        assert!(accounts.is_empty());
    }

    #[test]
    fn create_account_and_list() {
        let conn = setup();
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'bank','CNY',0,?3,?4,?5,?6,0)",
            rusqlite::params![id, "测试账户", now, now, 1, device_id()],
        ).unwrap();
        let accounts = list_accounts(&conn);
        assert!(accounts.iter().any(|a| a.id == id && a.name == "测试账户"));
    }

    #[test]
    fn delete_account_soft_deletes() {
        let conn = setup();
        let id = new_uuid();
        let now = now_iso();
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'cash','CNY',0,?3,?4,?5,?6,0)",
            rusqlite::params![id, "待删除", now, now, 1, device_id()],
        ).unwrap();
        assert!(list_accounts(&conn).iter().any(|a| a.id == id));
        conn.execute(
            "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
            rusqlite::params![id, now_iso(), device_id()],
        ).unwrap();
        assert!(!list_accounts(&conn).iter().any(|a| a.id == id));
    }

    #[test]
    fn balance_starts_at_initial() {
        let conn = setup();
        insert_account(&conn, "acc-bal-1", "现金", "cash", "CNY", 10000);
        assert_eq!(balance(&conn, "acc-bal-1"), 10000);
    }

    #[test]
    fn balance_adds_income() {
        let conn = setup();
        insert_account(&conn, "acc-bal-2", "现金", "cash", "CNY", 0);
        insert_tx(&conn, "tx1", "income", 5000, "acc-bal-2", None);
        assert_eq!(balance(&conn, "acc-bal-2"), 5000);
    }

    #[test]
    fn balance_subtracts_expense() {
        let conn = setup();
        insert_account(&conn, "acc-bal-3", "现金", "cash", "CNY", 10000);
        insert_tx(&conn, "tx2", "expense", 3000, "acc-bal-3", None);
        assert_eq!(balance(&conn, "acc-bal-3"), 7000);
    }

    #[test]
    fn balance_adds_transfer_in() {
        let conn = setup();
        insert_account(&conn, "acc-a", "账户A", "cash", "CNY", 0);
        insert_account(&conn, "acc-b", "账户B", "cash", "CNY", 0);
        insert_tx(&conn, "tx3", "transfer", 2000, "acc-a", Some("acc-b"));
        assert_eq!(balance(&conn, "acc-a"), -2000);
        assert_eq!(balance(&conn, "acc-b"), 2000);
    }

    #[test]
    fn balance_adds_refund() {
        let conn = setup();
        insert_account(&conn, "acc-bal-4", "现金", "cash", "CNY", 0);
        insert_tx(&conn, "tx4", "expense", 1000, "acc-bal-4", None);
        insert_tx(&conn, "tx5", "refund", 300, "acc-bal-4", None);
        assert_eq!(balance(&conn, "acc-bal-4"), -700);
    }

    #[test]
    fn soft_deleted_transaction_excluded_from_balance() {
        let conn = setup();
        insert_account(&conn, "acc-bal-5", "现金", "cash", "CNY", 0);
        insert_tx(&conn, "tx6", "income", 5000, "acc-bal-5", None);
        assert_eq!(balance(&conn, "acc-bal-5"), 5000);
        conn.execute(
            "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
            rusqlite::params!["tx6", now_iso(), device_id()],
        ).unwrap();
        assert_eq!(balance(&conn, "acc-bal-5"), 0);
    }

    #[test]
    fn list_account_balances_returns_all_accounts() {
        let conn = setup();
        insert_account(&conn, "acc-list-1", "现金", "cash", "CNY", 10000);
        insert_account(&conn, "acc-list-2", "储蓄卡", "bank", "CNY", 50000);
        insert_tx(&conn, "tx7", "income", 3000, "acc-list-1", None);
        insert_tx(&conn, "tx8", "expense", 2000, "acc-list-2", None);
        let accounts = list_accounts(&conn);
        assert_eq!(accounts.len(), 2);
        assert_eq!(balance(&conn, "acc-list-1"), 13000);
        assert_eq!(balance(&conn, "acc-list-2"), 48000);
    }
}
