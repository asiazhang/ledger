use rusqlite::{Connection, params};

use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::writer::{
    Input, NormalizedRow, insert_row, normalize, update_row,
};

use super::common::input;
use crate::tests::common::seed_category;
use tauri_app_lib::test_support;

fn read_row(conn: &Connection, id: &str) -> NormalizedRow {
    let row: RowFields = conn
        .query_row(
            "SELECT kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,merchant_id,policy_id,refund_of_transaction_id,note,date,\
             fx_rate_used,fx_rate_source \
             FROM transactions WHERE id=?1",
            params![id],
            |r| {
                Ok(RowFields {
                    kind: r.get(0)?,
                    amount_cents: r.get(1)?,
                    currency_code: r.get(2)?,
                    amount_native_cents: r.get(3)?,
                    account_id: r.get(4)?,
                    to_account_id: r.get(5)?,
                    category_id: r.get(6)?,
                    merchant_id: r.get(7)?,
                    policy_id: r.get(8)?,
                    refund_of_transaction_id: r.get(9)?,
                    note: r.get(10)?,
                    date: r.get(11)?,
                    fx_rate_used: r.get(12)?,
                    fx_rate_source: r.get(13)?,
                })
            },
        )
        .unwrap();
    NormalizedRow {
        funding: Vec::new(),
        kind: row.kind,
        amount_cents: row.amount_cents,
        currency_code: row.currency_code,
        amount_native_cents: row.amount_native_cents,
        fx_rate_used: row.fx_rate_used,
        fx_rate_source: row.fx_rate_source,
        account_id: row.account_id,
        to_account_id: row.to_account_id,
        category_id: row.category_id,
        policy_id: row.policy_id,
        merchant_id: row.merchant_id,
        refund_of_transaction_id: row.refund_of_transaction_id,
        funding_account_id: None,
        source_order_no: None,
        note: row.note,
        date: row.date,
        purchases: Vec::new(),
    }
}

struct RowFields {
    kind: TransactionKind,
    amount_cents: i64,
    currency_code: String,
    amount_native_cents: i64,
    account_id: Option<String>,
    to_account_id: Option<String>,
    category_id: Option<String>,
    merchant_id: Option<String>,
    policy_id: Option<String>,
    refund_of_transaction_id: Option<String>,
    note: Option<String>,
    date: String,
    fx_rate_used: Option<f64>,
    fx_rate_source: Option<tauri_app_lib::ledger_transaction::amount::FxRateSource>,
}

#[test]
fn insert_row_writes_full_row_and_generates_audit_fields() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &Input {
            note: Some("备注".into()),
            ..input(TransactionKind::Expense, 1234, "acc")
        },
    )
    .unwrap();

    let id = insert_row(&conn, &norm).unwrap();
    assert!(!id.is_empty());

    assert_eq!(read_row(&conn, &id), norm);

    let (created_at, updated_at, version, device_id, is_deleted): (String, String, i64, String, i64) =
        conn.query_row(
            "SELECT created_at,updated_at,version,device_id,is_deleted FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        created_at, updated_at,
        "新建行 created_at 与 updated_at 一致"
    );
    assert_eq!(version, 1);
    assert_eq!(
        device_id,
        ledger_sync_protocol::device::device_id(&conn).unwrap()
    );
    assert_eq!(is_deleted, 0);
}

#[test]
fn insert_row_generates_distinct_ids() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let norm = normalize(&conn, &input(TransactionKind::Income, 100, "acc")).unwrap();
    let id1 = insert_row(&conn, &norm).unwrap();
    let id2 = insert_row(&conn, &norm).unwrap();
    assert_ne!(id1, id2);
}

#[test]
fn update_row_overwrites_fields_and_bumps_version() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &Input {
            note: Some("旧备注".into()),
            ..input(TransactionKind::Expense, 500, "acc-a")
        },
    )
    .unwrap();
    let id = insert_row(&conn, &norm).unwrap();
    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();

    let updated = NormalizedRow {
        funding: Vec::new(),
        kind: TransactionKind::Transfer,
        amount_cents: 3000,
        currency_code: "CNY".into(),
        amount_native_cents: 3000,
        fx_rate_used: None,
        fx_rate_source: None,
        account_id: Some("acc-a".into()),
        to_account_id: Some("acc-b".into()),
        category_id: None,
        merchant_id: None,
        policy_id: None,
        refund_of_transaction_id: None,
        funding_account_id: None,
        source_order_no: None,
        note: Some("改后".into()),
        date: "2026-02-10".into(),
        purchases: Vec::new(),
    };
    update_row(&conn, &id, &updated).unwrap();

    assert_eq!(read_row(&conn, &id), updated);
    let (created_at_after, updated_at, version): (String, String, i64) = conn
        .query_row(
            "SELECT created_at,updated_at,version FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(created_at_after, created_at, "created_at 应保留");
    assert_eq!(version, 2, "version 应递增");
    assert!(!updated_at.is_empty(), "updated_at 应刷新");
}

#[test]
fn update_row_preserves_idempotent_identity() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let norm = normalize(&conn, &input(TransactionKind::Expense, 500, "acc")).unwrap();
    let id = insert_row(&conn, &norm).unwrap();
    conn.execute(
        "UPDATE transactions SET dedup_hash=?2, idempotency_key=?3 WHERE id=?1",
        params![id, "hash-abc", "row-1"],
    )
    .unwrap();

    update_row(
        &conn,
        &id,
        &NormalizedRow {
            amount_cents: 900,
            note: Some("改后".into()),
            ..norm
        },
    )
    .unwrap();

    let (key, hash): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT idempotency_key,dedup_hash FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(key.as_deref(), Some("row-1"), "幂等键应保留");
    assert_eq!(hash.as_deref(), Some("hash-abc"), "dedup_hash 应保留");
}

#[test]
fn normalize_insert_update_roundtrip() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let cat_food = seed_category(&conn, "餐饮", "expense");

    let created = normalize(
        &conn,
        &Input {
            category_id: Some(cat_food.clone()),
            note: Some("午餐".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    let id = insert_row(&conn, &created).unwrap();
    assert_eq!(read_row(&conn, &id), created);
    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();

    let modified = NormalizedRow {
        amount_cents: 1800,
        amount_native_cents: 1800,
        date: "2026-02-02".into(),
        ..created
    };
    update_row(&conn, &id, &modified).unwrap();
    assert_eq!(read_row(&conn, &id), modified);
    let (version, created_at_after): (i64, String) = conn
        .query_row(
            "SELECT version,created_at FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(version, 2);
    assert_eq!(created_at_after, created_at, "created_at 应保留");
}

#[test]
fn writer_rows_do_not_mark_dirty_entry_does() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);

    let row = normalize(&conn, &input(TransactionKind::Expense, 1500, "acc")).unwrap();
    let id = insert_row(&conn, &row).unwrap();
    assert!(
        !ledger_backup::get_state(&conn).unwrap().dirty,
        "Writer 落库本身不置脏（触发已上移写入口）"
    );
    update_row(&conn, &id, &row).unwrap();
    assert!(
        !ledger_backup::get_state(&conn).unwrap().dirty,
        "更新同样不置脏"
    );

    let conn = std::sync::Arc::new(std::sync::Mutex::new(test_support::open()));
    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        ledger_infra::db::write_locked(&guard, |conn| {
            test_support::seed_account(conn, "acc", "acc", "cash", "CNY", 0);
            let row = normalize(conn, &input(TransactionKind::Expense, 1500, "acc")).unwrap();
            let id = insert_row(conn, &row).unwrap();
            assert!(
                !ledger_backup::get_state(conn).unwrap().dirty,
                "提交点之前（闭包内）不置脏"
            );
            update_row(conn, &id, &row)
        })
        .unwrap();
    }
    let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        ledger_backup::get_state(&conn).unwrap().dirty,
        "写入口提交点应置脏"
    );
}
