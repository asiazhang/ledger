//! 行级写入语义：insert_row 全列映射与审计字段生成、update_row 字段覆盖与
//! 幂等身份保留、normalize → insert_row → update_row 端到端、置脏触发
//! （ADR-0032：已收口连接层统一写入口）。

use rusqlite::{Connection, params};

use crate::transaction::amount::TransactionKind;
use crate::transaction::writer::{Input, NormalizedRow, insert_row, normalize, update_row};

use super::common::{input, insert_account, insert_category, setup_db};

/// 读回一行交易的全部业务字段（与 insert_row 的列映射逐列比对）。
fn read_row(conn: &Connection, id: &str) -> NormalizedRow {
    // 命名字段而非长元组：读回列多，逐列命名可读性更好（也避免 clippy type_complexity）。
    let row: RowFields = conn
        .query_row(
            "SELECT kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,merchant_id,refund_of_transaction_id,note,date \
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
                    refund_of_transaction_id: r.get(8)?,
                    note: r.get(9)?,
                    date: r.get(10)?,
                })
            },
        )
        .unwrap();
    NormalizedRow {
        kind: row.kind,
        amount_cents: row.amount_cents,
        currency_code: row.currency_code,
        amount_native_cents: row.amount_native_cents,
        account_id: row.account_id,
        to_account_id: row.to_account_id,
        category_id: row.category_id,
        merchant_id: row.merchant_id,
        refund_of_transaction_id: row.refund_of_transaction_id,
        note: row.note,
        date: row.date,
    }
}

/// `read_row` 的中间读回结构（命名字段避免长元组）。
struct RowFields {
    kind: TransactionKind,
    amount_cents: i64,
    currency_code: String,
    amount_native_cents: i64,
    account_id: String,
    to_account_id: Option<String>,
    category_id: Option<String>,
    merchant_id: Option<String>,
    refund_of_transaction_id: Option<String>,
    note: Option<String>,
    date: String,
}

// ---------------------------------------------------------------------------
// insert_row：全列映射 + 审计字段
// ---------------------------------------------------------------------------

/// 落库后逐列读回比对：业务字段与归一化行一致，审计字段由模块生成
/// （version=1 / is_deleted=0 / created_at==updated_at / device_id 一致）。
#[test]
fn insert_row_writes_full_row_and_generates_audit_fields() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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

    // 业务字段全列映射正确
    assert_eq!(read_row(&conn, &id), norm);

    // 审计字段
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
    assert_eq!(device_id, crate::db::device_id());
    assert_eq!(is_deleted, 0);
}

/// 两次 insert 生成互异的 id。
#[test]
fn insert_row_generates_distinct_ids() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let norm = normalize(&conn, &input(TransactionKind::Income, 100, "acc")).unwrap();
    let id1 = insert_row(&conn, &norm).unwrap();
    let id2 = insert_row(&conn, &norm).unwrap();
    assert_ne!(id1, id2);
}

// ---------------------------------------------------------------------------
// update_row：字段覆盖 + 幂等身份保留 + 版本递增
// ---------------------------------------------------------------------------

/// update 覆盖全部可编辑字段，保留 id / created_at，version 递增。
#[test]
fn update_row_overwrites_fields_and_bumps_version() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    insert_account(&conn, "acc-b", "CNY");
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
        kind: TransactionKind::Transfer,
        amount_cents: 3000,
        currency_code: "CNY".into(),
        amount_native_cents: 3000,
        account_id: "acc-a".into(),
        to_account_id: Some("acc-b".into()),
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: Some("改后".into()),
        date: "2026-02-10".into(),
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

/// update 保留幂等身份（idempotency_key / dedup_hash，由命令层回写、本模块不触碰）。
#[test]
fn update_row_preserves_idempotent_identity() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let norm = normalize(&conn, &input(TransactionKind::Expense, 500, "acc")).unwrap();
    let id = insert_row(&conn, &norm).unwrap();
    // 模拟批量导入回写幂等身份（与 batch 模块落库后 UPDATE 同构）
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

// ---------------------------------------------------------------------------
// 端到端：normalize → insert_row → update_row
// ---------------------------------------------------------------------------

/// 创建再修改一笔交易：归一化 → 落库 → 读回 → 更新 → 读回，全链路一致。
#[test]
fn normalize_insert_update_roundtrip() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_category(&conn, "cat-food");

    let created = normalize(
        &conn,
        &Input {
            category_id: Some("cat-food".into()),
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

// ---------------------------------------------------------------------------
// 置脏触发（ADR-0032：已收口连接层统一写入口）
// ---------------------------------------------------------------------------

/// Writer 落库本身对备份域零感知（ADR-0032）：insert_row / update_row 不再自带
/// 置脏；同样的落库经连接层写入口 `db.write` 执行（命令层真实形态）时，
/// 由提交点单点置脏。
#[test]
fn writer_rows_do_not_mark_dirty_entry_does() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");

    let row = normalize(&conn, &input(TransactionKind::Expense, 1500, "acc")).unwrap();
    let id = insert_row(&conn, &row).unwrap();
    assert!(
        !crate::auto_backup::get_state(&conn).unwrap().dirty,
        "Writer 落库本身不置脏（触发已上移写入口）"
    );
    update_row(&conn, &id, &row).unwrap();
    assert!(
        !crate::auto_backup::get_state(&conn).unwrap().dirty,
        "更新同样不置脏"
    );

    // 经写入口执行同样的落库（与 IPC 命令同形态）→ 提交点置脏，且置脏是幂等
    // 标记、不做「已脏跳过」优化。
    let state = crate::db::DbState::open_in_memory().unwrap();
    state
        .write(|conn| {
            insert_account(conn, "acc", "CNY");
            let row = normalize(conn, &input(TransactionKind::Expense, 1500, "acc")).unwrap();
            let id = insert_row(conn, &row).unwrap();
            assert!(
                !crate::auto_backup::get_state(conn).unwrap().dirty,
                "提交点之前（闭包内）不置脏"
            );
            update_row(conn, &id, &row)
        })
        .unwrap();
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        crate::auto_backup::get_state(&conn).unwrap().dirty,
        "写入口提交点应置脏"
    );
}
