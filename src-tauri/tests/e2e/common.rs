use rusqlite::Connection;
use rusqlite::params;

use tauri_app_lib::db::new_uuid;
use tauri_app_lib::models::Transaction;

use crate::world::LedgerWorld;

/// 断言最近一次操作记录的错误信息包含指定片段（多个 `*_steps` 模块共用的 seam 断言）。
pub fn assert_last_error_contains(world: &LedgerWorld, needle: &str) {
    match &world.last_error {
        Some(msg) => assert!(
            msg.contains(needle),
            "错误消息不匹配: 期望包含 '{needle}', 实际 '{msg}'"
        ),
        None => panic!("预期错误但未发生"),
    }
}

/// 在数据库中插入账户用于测试。
pub fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name, kind, currency],
    )
    .unwrap();
}

/// 生成 UUID v7 作为账户 ID。
pub fn new_account_id() -> String {
    new_uuid()
}

/// 查询全部未删除交易，按日期倒序（与 `list_transactions_internal` 的确定性排序一致，id 为 tiebreaker）。
pub fn query_all_transactions(conn: &Connection) -> Vec<Transaction> {
    let mut stmt = conn
        .prepare(
            "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
             to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,\
             version,device_id,is_deleted \
             FROM transactions WHERE is_deleted=0 ORDER BY date DESC, created_at DESC, id DESC",
        )
        .unwrap();
    stmt.query_map([], |r| {
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
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// 查询所有未删除、未隐藏账户名称（用户侧视角）。
pub fn query_accounts_by_name(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM accounts WHERE is_deleted=0 AND is_hidden=0 ORDER BY created_at")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}
