//! `TransactionBatch` 模块的单元测试：断言**外部行为**——`run` 的返回值
//! （success / duplicate / id / error）与实际落库行数/内容；不断言内部实现
//! （事务 BEGIN/COMMIT 写法、SQL 字符串、去重分支结构）。
//!
//! 原命令模块中批量写入/`compute_dedup_hash` 相关测试随重构迁入本模块（issue #53 / #63 / #66），改用
//! `TransactionBatch::run` 断言外部行为；`transactions` 模块遗留的旧 `batch_*`
//! 直调 `create_transaction_internal` 测试（全部有效落库/转账缺目标账户/零金额）已随
//! #66 处理——零金额校验迁入本模块以 `run` 外部行为覆盖，其余被本模块既有
//! 测试与 `transaction::writer` 模块测试共同取代（通用 kind 归一化语义已收口到
//! Writer 接缝）。单条写入
//! （`create_transaction_internal`）与删除/修改（`delete_transaction_internal`/
//! `update_transaction_internal`）的测试仍留在 `transactions` 模块。

use super::*;
use rusqlite::Connection;

use crate::commands::transactions::{delete_transaction_internal, update_transaction_internal};
use crate::db::{device_id, init_db, now_iso, open_in_memory};
use crate::test_utils::{CapturedEvent, capture_events};
use crate::transaction::amount::TransactionKind;
use rusqlite::params;
use tracing::Level;

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

fn make_input(
    account_id: &str,
    kind: TransactionKind,
    amount: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
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

#[test]
fn dedup_hash_is_stable_for_same_fields() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let a = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let b = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    assert_eq!(compute_dedup_hash(&a), compute_dedup_hash(&b));
}

#[test]
fn dedup_hash_excludes_note_and_category() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let base = make_input("acc-dedup", TransactionKind::Expense, 500, "2026-07-02");
    let with_note = TransactionInput {
        note: Some("备注".into()),
        ..base.clone()
    };
    let with_category = TransactionInput {
        category_id: Some("cat-1".into()),
        ..base.clone()
    };
    let h = compute_dedup_hash(&base);
    assert_eq!(compute_dedup_hash(&with_note), h);
    assert_eq!(compute_dedup_hash(&with_category), h);
}

#[test]
fn dedup_hash_changes_when_content_fields_change() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let base = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let h = compute_dedup_hash(&base);
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-dedup",
            TransactionKind::Income,
            2000,
            "2026-07-01"
        )),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-dedup",
            TransactionKind::Expense,
            1000,
            "2026-07-01"
        )),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-other",
            TransactionKind::Income,
            1000,
            "2026-07-01"
        )),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input(
            "acc-dedup",
            TransactionKind::Income,
            1000,
            "2026-07-02"
        )),
        h
    );
}

#[test]
fn dedup_hash_pins_empty_to_account_id_as_empty_string() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");
    let no_to = make_input("acc-dedup", TransactionKind::Transfer, 3000, "2026-07-03");
    let empty_to = TransactionInput {
        to_account_id: Some("".into()),
        ..no_to.clone()
    };
    assert_eq!(
        compute_dedup_hash(&no_to),
        compute_dedup_hash(&empty_to),
        "缺省 to_account_id 应等同空串"
    );
    let with_to = TransactionInput {
        to_account_id: Some("acc-to".into()),
        ..no_to.clone()
    };
    assert_ne!(
        compute_dedup_hash(&no_to),
        compute_dedup_hash(&with_to),
        "指定 to_account_id 应改变哈希"
    );
}

#[test]
fn dedup_hash_matches_known_sha256_vector() {
    let conn = setup();
    insert_account(&conn, "acc-1", "现金", "cash", "CNY");
    let input = make_input("acc-1", TransactionKind::Income, 1000, "2026-07-01");
    // sha256("2026-07-01|income|1000|CNY|acc-1|")
    assert_eq!(
        compute_dedup_hash(&input),
        "d5a4ee5fa04913672a319a06c454283d74d312f13506a27fc81c72b09602a558"
    );
}

#[test]
fn batch_create_marks_duplicates_and_keeps_rows() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

    let inputs = vec![
        make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01"),
        make_input("acc-dedup", TransactionKind::Expense, 500, "2026-07-02"),
    ];
    let first = TransactionBatch::run(&conn, inputs.clone(), true).unwrap();
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|r| r.success && !r.duplicate && r.id.is_some())
    );

    let second = TransactionBatch::run(&conn, inputs, true).unwrap();
    assert_eq!(second.len(), 2);
    assert!(
        second
            .iter()
            .all(|r| r.success && r.duplicate && r.id.is_none())
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn batch_create_with_dedup_false_writes_duplicates() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

    let inputs = vec![make_input(
        "acc-dedup",
        TransactionKind::Income,
        1000,
        "2026-07-01",
    )];
    TransactionBatch::run(&conn, inputs.clone(), true).unwrap();
    let second = TransactionBatch::run(&conn, inputs, false).unwrap();
    assert_eq!(second.len(), 1);
    assert!(second[0].success && !second[0].duplicate && second[0].id.is_some());

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn dedup_ignores_soft_deleted_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-dedup", "现金", "cash", "CNY");

    let input = make_input("acc-dedup", TransactionKind::Income, 1000, "2026-07-01");
    let first = TransactionBatch::run(&conn, vec![input.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id()],
    ).unwrap();

    let second = TransactionBatch::run(&conn, vec![input], true).unwrap();
    assert!(second[0].success && !second[0].duplicate && second[0].id.is_some());

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn batch_create_idempotency_key_rerun_skips_and_returns_id() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].success && !first[0].duplicate, "首次导入应新写入");
    let id1 = first[0].id.clone().unwrap();

    let second = TransactionBatch::run(&conn, vec![a], true).unwrap();
    assert_eq!(second.len(), 1);
    assert!(
        second[0].success && second[0].duplicate,
        "同幂等键重跑应去重跳过"
    );
    assert_eq!(
        second[0].id.as_deref(),
        Some(id1.as_str()),
        "同键重跑应返回该笔已有 id"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "重复导入不应新增交易");
}

#[test]
fn batch_create_idempotency_key_content_agnostic() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();

    // 同一幂等键、本轮内容不同：仍应按同一条跳过（内容无关）。
    let mut b = make_input("acc-key", TransactionKind::Expense, 2000, "2026-02-01");
    b.idempotency_key = Some("file:1:1".into());
    let second = TransactionBatch::run(&conn, vec![b], true).unwrap();
    assert!(
        second[0].success && second[0].duplicate,
        "同键不同内容仍应去重跳过"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "内容无关：即便内容变化也不新增");
}

#[test]
fn batch_create_idempotency_key_different_keys_same_content_keeps_both() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let mut b = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    b.idempotency_key = Some("file:2:1".into());
    let r = TransactionBatch::run(&conn, vec![a, b], true).unwrap();
    assert_eq!(r.len(), 2);
    assert!(
        r.iter().all(|x| x.success && !x.duplicate),
        "不同键但内容完全相同应都保留"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "内容相同的两笔独立交易都应落库");
}

#[test]
fn batch_create_idempotency_key_same_key_dedup_false_raises_constraint() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("dup-key".into());
    let mut b = make_input("acc-key", TransactionKind::Income, 2000, "2026-01-02");
    b.idempotency_key = Some("dup-key".into());

    // dedup=false 直接落库两笔同键：部分唯一索引应拒绝（一键至多一活交易）。
    let err = TransactionBatch::run(&conn, vec![a, b], false).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("unique"),
        "同键重复应触发唯一索引约束，实际: {err:?}"
    );

    // 提交失败整批回滚（外部可观察结果）：触发约束的行与同批已写行都不落库。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "批次失败应整批回滚，不残留任何行");
}

#[test]
fn idempotency_key_dedup_query_uses_partial_index() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");
    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    TransactionBatch::run(&conn, vec![a], true).unwrap();

    // EXPLAIN QUERY PLAN 的 detail 列（第 4 列，索引 3）应命中部分唯一索引，而非全表扫描。
    let mut stmt = conn
        .prepare(
            "EXPLAIN QUERY PLAN \
             SELECT id FROM transactions \
             WHERE idempotency_key=?1 AND is_deleted=0 LIMIT 1",
        )
        .unwrap();
    let details: Vec<String> = stmt
        .query_map(rusqlite::params!["file:1:1"], |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    let plan = details.join(" | ");
    assert!(
        plan.contains("idx_transactions_idempotency_key"),
        "幂等键去重查询应命中部分唯一索引: {plan}"
    );
}

#[test]
fn batch_create_idempotency_key_soft_deleted_frees_slot() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");

    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();
    delete_transaction_internal(&conn, &id).unwrap();

    // 软删除后同键重跑：部分唯一索引只约束未删除交易，应重新写入。
    let second = TransactionBatch::run(&conn, vec![a], true).unwrap();
    assert!(
        second[0].success && !second[0].duplicate && second[0].id.is_some(),
        "软删除后同键应重新写入"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

// ---- dedup_identity 判定函数直接覆盖（issue #62）----

#[test]
fn dedup_identity_key_hit_returns_existing_id() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    // 落库一笔带幂等键的交易。
    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let created = TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();
    let existing_id = created[0].id.clone().unwrap();

    // 同键但内容不同：内容无关，仍按幂等键命中并回传已有 id。
    let mut b = make_input("acc-ident", TransactionKind::Expense, 2000, "2026-02-01");
    b.idempotency_key = Some("file:1:1".into());
    match dedup_identity(&conn, &b).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(
                id.as_deref(),
                Some(existing_id.as_str()),
                "幂等键命中应回传已有 id"
            );
        }
        other => panic!("同键应判定为命中已有，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_hash_hit_returns_none() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    let a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();

    // 无键同内容：内容哈希兜底命中，冻结契约回传 id:None（不回归）。
    match dedup_identity(&conn, &a).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(id, None, "内容哈希命中应回传 id:None（冻结契约，不回归）");
        }
        other => panic!("同内容应判定为命中已有，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_new_for_fresh_row() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    let a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    TransactionBatch::run(&conn, vec![a], true).unwrap();

    // 内容不同（且无键）：应判定为新写，且携带与落库回写一致的内容哈希。
    let fresh = make_input("acc-ident", TransactionKind::Expense, 2000, "2026-02-01");
    match dedup_identity(&conn, &fresh).unwrap() {
        DedupIdentity::New { dedup_hash } => {
            assert_eq!(
                dedup_hash,
                compute_dedup_hash(&fresh),
                "New 应携带与落库回写一致的内容哈希"
            );
        }
        other => panic!("内容不同应判定为新写，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_key_takes_precedence_over_content_hash() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    // 两笔内容完全相同但幂等键不同的交易（内容哈希相同）：不同键都应保留。
    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let mut b = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    b.idempotency_key = Some("file:2:1".into());
    let r = TransactionBatch::run(&conn, vec![a.clone(), b], true).unwrap();
    let id_a = r[0].id.clone().unwrap();

    // 无键、内容与二者相同：内容哈希命中（回传 id:None，冻结契约）。
    let keyless = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    match dedup_identity(&conn, &keyless).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(id, None, "内容哈希命中应回传 id:None");
        }
        other => panic!("同内容无键应命中已有，实际: {other:?}"),
    }

    // 同键 file:1:1 但内容不同：幂等键命中，回传 a 的 id（内容无关）。
    let mut c = make_input("acc-ident", TransactionKind::Expense, 2000, "2026-02-01");
    c.idempotency_key = Some("file:1:1".into());
    match dedup_identity(&conn, &c).unwrap() {
        DedupIdentity::Existing { id } => {
            assert_eq!(
                id.as_deref(),
                Some(id_a.as_str()),
                "同键命中应回传 file:1:1 那笔的 id"
            );
        }
        other => panic!("同键应命中已有，实际: {other:?}"),
    }
}

#[test]
fn dedup_identity_ignores_soft_deleted_rows() {
    let conn = setup();
    insert_account(&conn, "acc-ident", "现金", "cash", "CNY");

    // 带键落库后软删除：键路径与哈希路径都不应命中。
    let mut a = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let created = TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();
    let id = created[0].id.clone().unwrap();
    delete_transaction_internal(&conn, &id).unwrap();

    match dedup_identity(&conn, &a).unwrap() {
        DedupIdentity::New { .. } => {}
        other => panic!("软删除后同键应判定为新写，实际: {other:?}"),
    }
    // 无键路径同样忽略软删除行。
    let keyless = make_input("acc-ident", TransactionKind::Income, 1000, "2026-01-01");
    match dedup_identity(&conn, &keyless).unwrap() {
        DedupIdentity::New { .. } => {}
        other => panic!("软删除后同内容应判定为新写，实际: {other:?}"),
    }
}

#[test]
fn batch_create_idempotency_key_buy_sell_different_instruments_kept() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv-key", "inst-aapl");
    // 第二个不同标的（相同币种 USD）。
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'MSFT','stock','Msft','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params!["inst-msft"],
    )
    .unwrap();

    // 两笔买入：不同标的、相同原始金额字段（amount_cents=0，内容哈希盲区），带键应都保留。
    let mut buy1 = make_buy_input("acc-inv-key", "inst-aapl", 10.0, 10000, 500);
    buy1.idempotency_key = Some("file:1:1".into());
    let mut buy2 = make_buy_input("acc-inv-key", "inst-msft", 5.0, 20000, 300);
    buy2.idempotency_key = Some("file:1:2".into());

    let r = TransactionBatch::run(&conn, vec![buy1, buy2], true).unwrap();
    assert_eq!(r.len(), 2);
    assert!(
        r.iter().all(|x| x.success && !x.duplicate),
        "不同标的(带键)都应保留，不应被内容哈希误去重"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn delete_transaction_internal_frees_dedup_slot_for_reimport() {
    let conn = setup();
    insert_account(&conn, "acc-reimport", "现金", "cash", "CNY");

    let input = make_input("acc-reimport", TransactionKind::Income, 1000, "2026-07-01");
    let first = TransactionBatch::run(&conn, vec![input.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();

    delete_transaction_internal(&conn, &id).unwrap();

    let second = TransactionBatch::run(&conn, vec![input], true).unwrap();
    assert!(
        second[0].success && !second[0].duplicate && second[0].id.is_some(),
        "删除后重跑应重新写入（duplicate=false）"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        idempotency_key: None,
    }
}

fn setup_investment_account(conn: &Connection, account_id: &str, instrument_id: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'SYM','stock','Symbol','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![instrument_id],
    )
    .unwrap();
    // buy/sell 本位币折算走 Amount 接缝（issue #70）：补 1:1 汇率，非默认币种账户交易不报缺汇率。
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-fix','USD','CNY',1.0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();
}

#[test]
fn update_transaction_internal_preserves_key_and_rerun_dedup() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");
    let mut a = make_input("acc-key", TransactionKind::Income, 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = TransactionBatch::run(&conn, vec![a.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();

    // 编辑内容（金额/备注/日期），幂等键应保持不变。
    let mut edited = make_input("acc-key", TransactionKind::Income, 2000, "2026-01-03");
    edited.note = Some("改".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let key: Option<String> = conn
        .query_row(
            "SELECT idempotency_key FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(key.as_deref(), Some("file:1:1"), "编辑不应改变幂等键");

    // 编辑后重跑同批导入（带同键）：仍按同键去重、返回已有 id → 不产生重复。
    let mut rerun = make_input("acc-key", TransactionKind::Income, 3000, "2026-02-01");
    rerun.idempotency_key = Some("file:1:1".into());
    let second = TransactionBatch::run(&conn, vec![rerun], true).unwrap();
    assert!(
        second[0].success && second[0].duplicate,
        "同键重跑应去重跳过"
    );
    assert_eq!(second[0].id.as_deref(), Some(id.as_str()));

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "编辑后重跑不应新增交易");
}

// ---------------------------------------------------------------------------
// 导入批次汇总日志（ADR-0009 决策 #5 / issue #45）
// ---------------------------------------------------------------------------

/// 从事件字段里取同名 key 的值（无则 None）。
fn field_value<'a>(event: &'a CapturedEvent, key: &str) -> Option<&'a str> {
    event
        .fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// 从捕获的事件里找出批次汇总事件（唯一带 `total`+`failed` 字段的事件）。
fn find_batch_summary(events: &[CapturedEvent]) -> Option<&CapturedEvent> {
    events.iter().find(|e| {
        e.fields.iter().any(|(k, _)| k == "total") && e.fields.iter().any(|(k, _)| k == "failed")
    })
}

/// 成功批次：汇总行以 info 级别出现，含总耗时与条数（失败数=0）。
#[test]
fn batch_create_logs_summary_on_success() {
    let conn = setup();
    insert_account(&conn, "acc-log-ok", "现金", "cash", "CNY");

    let inputs = vec![
        make_input("acc-log-ok", TransactionKind::Income, 1000, "2026-07-01"),
        make_input("acc-log-ok", TransactionKind::Expense, 500, "2026-07-02"),
    ];
    let events = capture_events(|| {
        let r = TransactionBatch::run(&conn, inputs, true).unwrap();
        assert_eq!(r.len(), 2);
    });

    let summary = find_batch_summary(&events).expect("应有一条批次汇总日志");
    assert_eq!(summary.level, Level::INFO, "汇总行应为 info 级（默认可见）");
    assert_eq!(field_value(summary, "total"), Some("2"), "交易条数应为 2");
    assert_eq!(
        field_value(summary, "failed"),
        Some("0"),
        "全成功时失败数应为 0"
    );
    assert!(
        field_value(summary, "elapsed_ms").is_some(),
        "汇总行应含总耗时"
    );
}

/// 批次中途回滚：汇总行仍出现，且含失败条数（触发唯一约束回滚的那条）。
#[test]
fn batch_create_logs_summary_on_rollback() {
    let conn = setup();
    insert_account(&conn, "acc-log-rb", "现金", "cash", "CNY");

    let mut a = make_input("acc-log-rb", TransactionKind::Income, 1000, "2026-07-01");
    a.idempotency_key = Some("dup-rb".into());
    let mut b = make_input("acc-log-rb", TransactionKind::Income, 2000, "2026-07-02");
    b.idempotency_key = Some("dup-rb".into());

    let events = capture_events(|| {
        let err = TransactionBatch::run(&conn, vec![a, b], false).unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("unique"),
            "同键重复应触发唯一索引约束，实际: {err:?}"
        );
    });

    let summary = find_batch_summary(&events).expect("回滚后仍应有批次汇总日志");
    assert_eq!(summary.level, Level::INFO);
    assert_eq!(field_value(summary, "total"), Some("2"));
    assert_eq!(field_value(summary, "failed"), Some("1"), "应含失败条数");
    assert!(field_value(summary, "elapsed_ms").is_some());
}

/// 部分行无效但批次提交成功：汇总行含非零失败条数（有效行落库、无效行跳过）。
#[test]
fn batch_create_logs_failed_count_with_invalid_row() {
    let conn = setup();
    insert_account(&conn, "acc-log-part", "现金", "cash", "CNY");

    // 失败行：转账未指定目标账户 → Invalid；成功行：普通收入。
    let inputs = vec![
        make_input(
            "acc-log-part",
            TransactionKind::Transfer,
            1000,
            "2026-07-01",
        ),
        make_input("acc-log-part", TransactionKind::Income, 1000, "2026-07-02"),
    ];
    let events = capture_events(|| {
        let r = TransactionBatch::run(&conn, inputs, false).unwrap();
        assert_eq!(r.len(), 2);
        assert!(!r[0].success, "转账未指定目标账户应失败");
        assert!(!r[0].duplicate && r[0].id.is_none(), "失败行应无 id");
        let msg = r[0].error.as_deref().expect("失败行应带 error");
        assert!(
            msg.contains("目标账户"),
            "错误信息应说明转账约束，实际: {msg}"
        );
        assert!(
            r[1].success && !r[1].duplicate && r[1].id.is_some(),
            "同批有效行应正常落库"
        );
    });

    let summary = find_batch_summary(&events).expect("应有一条批次汇总日志");
    assert_eq!(summary.level, Level::INFO);
    assert_eq!(field_value(summary, "total"), Some("2"));
    assert_eq!(field_value(summary, "failed"), Some("1"), "应含失败条数");

    // 单条校验失败不影响同批（外部可观察结果）：仅有效行落库。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "无效行不落库，有效行正常落库");
}

/// 零金额行：单条校验失败（金额必须大于 0），不影响同批其他行落库（issue #66
/// 迁自 `transactions` 模块旧 `batch_rejects_zero_amount`，改经 `run` 断言）。
#[test]
fn batch_create_zero_amount_row_isolated() {
    let conn = setup();
    insert_account(&conn, "acc-log-zero", "现金", "cash", "CNY");

    let inputs = vec![
        TransactionInput {
            amount_cents: 0,
            ..make_input("acc-log-zero", TransactionKind::Income, 100, "2026-07-01")
        },
        make_input("acc-log-zero", TransactionKind::Income, 1000, "2026-07-02"),
    ];
    let r = TransactionBatch::run(&conn, inputs, false).unwrap();
    assert_eq!(r.len(), 2);
    assert!(!r[0].success, "零金额应校验失败");
    assert!(!r[0].duplicate && r[0].id.is_none(), "失败行应无 id");
    let msg = r[0].error.as_deref().expect("失败行应带 error");
    assert!(
        msg.contains("大于 0"),
        "错误信息应说明金额约束，实际: {msg}"
    );
    assert!(
        r[1].success && !r[1].duplicate && r[1].id.is_some(),
        "同批有效行应正常落库"
    );

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "仅有效行落库");
}
