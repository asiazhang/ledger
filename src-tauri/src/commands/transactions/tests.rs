use super::*;
use rusqlite::Connection;

use crate::db::{device_id, init_db, now_iso, open_in_memory};
use crate::test_utils::{CapturedEvent, capture_events};
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
        idempotency_key: None,
    }
}

#[test]
fn dedup_hash_is_stable_for_same_fields() {
    let a = make_input("acc-dedup", "income", 1000, "2026-07-01");
    let b = make_input("acc-dedup", "income", 1000, "2026-07-01");
    assert_eq!(compute_dedup_hash(&a), compute_dedup_hash(&b));
}

#[test]
fn dedup_hash_excludes_note_and_category() {
    let base = make_input("acc-dedup", "expense", 500, "2026-07-02");
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
    let base = make_input("acc-dedup", "income", 1000, "2026-07-01");
    let h = compute_dedup_hash(&base);
    assert_ne!(
        compute_dedup_hash(&make_input("acc-dedup", "income", 2000, "2026-07-01")),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input("acc-dedup", "expense", 1000, "2026-07-01")),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input("acc-other", "income", 1000, "2026-07-01")),
        h
    );
    assert_ne!(
        compute_dedup_hash(&make_input("acc-dedup", "income", 1000, "2026-07-02")),
        h
    );
}

#[test]
fn dedup_hash_pins_empty_to_account_id_as_empty_string() {
    let no_to = make_input("acc-dedup", "transfer", 3000, "2026-07-03");
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
    let input = make_input("acc-1", "income", 1000, "2026-07-01");
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
        make_input("acc-dedup", "income", 1000, "2026-07-01"),
        make_input("acc-dedup", "expense", 500, "2026-07-02"),
    ];
    let first = create_transactions_internal(&conn, inputs.clone(), true).unwrap();
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|r| r.success && !r.duplicate && r.id.is_some())
    );

    let second = create_transactions_internal(&conn, inputs, true).unwrap();
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

    let inputs = vec![make_input("acc-dedup", "income", 1000, "2026-07-01")];
    create_transactions_internal(&conn, inputs.clone(), true).unwrap();
    let second = create_transactions_internal(&conn, inputs, false).unwrap();
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

    let input = make_input("acc-dedup", "income", 1000, "2026-07-01");
    let first = create_transactions_internal(&conn, vec![input.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id()],
    ).unwrap();

    let second = create_transactions_internal(&conn, vec![input], true).unwrap();
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

    let mut a = make_input("acc-key", "income", 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = create_transactions_internal(&conn, vec![a.clone()], true).unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].success && !first[0].duplicate, "首次导入应新写入");
    let id1 = first[0].id.clone().unwrap();

    let second = create_transactions_internal(&conn, vec![a], true).unwrap();
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

    let mut a = make_input("acc-key", "income", 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    create_transactions_internal(&conn, vec![a.clone()], true).unwrap();

    // 同一幂等键、本轮内容不同：仍应按同一条跳过（内容无关）。
    let mut b = make_input("acc-key", "expense", 2000, "2026-02-01");
    b.idempotency_key = Some("file:1:1".into());
    let second = create_transactions_internal(&conn, vec![b], true).unwrap();
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

    let mut a = make_input("acc-key", "income", 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let mut b = make_input("acc-key", "income", 1000, "2026-01-01");
    b.idempotency_key = Some("file:2:1".into());
    let r = create_transactions_internal(&conn, vec![a, b], true).unwrap();
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

    let mut a = make_input("acc-key", "income", 1000, "2026-01-01");
    a.idempotency_key = Some("dup-key".into());
    let mut b = make_input("acc-key", "income", 2000, "2026-01-02");
    b.idempotency_key = Some("dup-key".into());

    // dedup=false 直接落库两笔同键：部分唯一索引应拒绝（一键至多一活交易）。
    let err = create_transactions_internal(&conn, vec![a, b], false).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("unique"),
        "同键重复应触发唯一索引约束，实际: {err:?}"
    );
}

#[test]
fn idempotency_key_dedup_query_uses_partial_index() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");
    let mut a = make_input("acc-key", "income", 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    create_transactions_internal(&conn, vec![a], true).unwrap();

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

    let mut a = make_input("acc-key", "income", 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = create_transactions_internal(&conn, vec![a.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();
    delete_transaction_internal(&conn, &id).unwrap();

    // 软删除后同键重跑：部分唯一索引只约束未删除交易，应重新写入。
    let second = create_transactions_internal(&conn, vec![a], true).unwrap();
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

    let r = create_transactions_internal(&conn, vec![buy1, buy2], true).unwrap();
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

#[test]
fn create_income_and_expense_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-crud", "现金", "cash", "CNY");

    let id1 =
        insert_transaction(&conn, make_input("acc-crud", "income", 5000, "2026-02-01")).unwrap();
    let id2 = insert_transaction(
        &conn,
        TransactionInput {
            amount_cents: 1500,
            note: Some("午餐".into()),
            category_id: None,
            ..make_input("acc-crud", "expense", 100, "2026-02-02")
        },
    )
    .unwrap();
    assert_ne!(id1, id2);
    let row1: (String, String, i64, Option<String>) = conn
        .query_row(
            "SELECT kind, account_id, amount_cents, note FROM transactions WHERE id=?1",
            params![id1],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row1.0, "income");
    assert_eq!(row1.2, 5000);
}

#[test]
fn create_transfer_with_to_account() {
    let conn = setup();
    insert_account(&conn, "acc-from", "A账户", "cash", "CNY");
    insert_account(&conn, "acc-to", "B账户", "cash", "CNY");

    let id = insert_transaction(
        &conn,
        TransactionInput {
            kind: "transfer".into(),
            amount_cents: 3000,
            currency_code: "CNY".into(),
            account_id: "acc-from".into(),
            to_account_id: Some("acc-to".into()),
            date: "2026-03-01".into(),
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap();
    let (kind, from, to): (String, String, Option<String>) = conn
        .query_row(
            "SELECT kind, account_id, to_account_id FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(kind, "transfer");
    assert_eq!(from, "acc-from");
    assert_eq!(to.as_deref(), Some("acc-to"));
}

#[test]
fn list_transactions_ordered_by_date_desc() {
    let conn = setup();
    insert_account(&conn, "acc-list", "现金", "cash", "CNY");

    insert_transaction(&conn, make_input("acc-list", "income", 100, "2026-01-03")).unwrap();
    insert_transaction(&conn, make_input("acc-list", "income", 200, "2026-01-01")).unwrap();
    insert_transaction(&conn, make_input("acc-list", "income", 300, "2026-01-02")).unwrap();

    let rows: Vec<(String, i64)> = conn
        .prepare(
            "SELECT kind, amount_cents FROM transactions WHERE is_deleted=0 \
             ORDER BY date DESC, created_at DESC",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].1, 100); // 01-03 first
    assert_eq!(rows[1].1, 300); // 01-02
    assert_eq!(rows[2].1, 200); // 01-01 last
}

#[test]
fn list_transactions_limit() {
    let conn = setup();
    insert_account(&conn, "acc-limit", "现金", "cash", "CNY");

    insert_transaction(&conn, make_input("acc-limit", "income", 100, "2026-01-01")).unwrap();
    insert_transaction(&conn, make_input("acc-limit", "income", 200, "2026-01-02")).unwrap();
    insert_transaction(&conn, make_input("acc-limit", "income", 300, "2026-01-03")).unwrap();

    let rows: Vec<i64> = conn
        .prepare(
            "SELECT amount_cents FROM transactions WHERE is_deleted=0 \
             ORDER BY date DESC, created_at DESC LIMIT 2",
        )
        .unwrap()
        .query_map([], |r| r.get::<_, i64>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 2);
}

/// 把所有交易的时间戳改为同一值，模拟"同一批导入每批一个时间戳"。
fn set_created_at(conn: &Connection, created_at: &str) {
    conn.execute(
        "UPDATE transactions SET created_at=?1, updated_at=?1",
        params![created_at],
    )
    .unwrap();
}

#[test]
fn list_transactions_pagination_returns_page_and_total() {
    let conn = setup();
    insert_account(&conn, "acc-page", "现金", "cash", "CNY");

    for i in 1..=25 {
        insert_transaction(
            &conn,
            make_input("acc-page", "expense", i * 100, &format!("2026-01-{:02}", i)),
        )
        .unwrap();
    }

    let p1 = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            page: Some(1),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p1.items.len(), 10, "第 1 页应返回 10 条");
    assert_eq!(p1.total, 25, "total 应为过滤后总数");

    let p3 = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            page: Some(3),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p3.items.len(), 5, "最后一页应返回剩余条数");
    assert_eq!(p3.total, 25);
}

#[test]
fn list_transactions_pagination_total_respects_filters() {
    let conn = setup();
    insert_account(&conn, "acc-f1", "现金", "cash", "CNY");
    insert_account(&conn, "acc-f2", "银行", "bank", "CNY");

    for i in 1..=8 {
        insert_transaction(
            &conn,
            make_input("acc-f1", "expense", i * 100, &format!("2026-02-{:02}", i)),
        )
        .unwrap();
    }
    insert_transaction(&conn, make_input("acc-f2", "income", 9000, "2026-02-09")).unwrap();
    insert_transaction(&conn, make_input("acc-f1", "income", 1000, "2026-02-10")).unwrap();

    let by_account = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            account_id: Some("acc-f1".into()),
            page: Some(1),
            page_size: Some(5),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_account.items.len(), 5);
    assert_eq!(by_account.total, 9, "total 应按过滤后计数");

    let by_kind = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            kind: Some("income".into()),
            page: Some(1),
            page_size: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_kind.items.len(), 1);
    assert_eq!(by_kind.total, 2);

    let by_date = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            from: Some("2026-02-03".into()),
            to: Some("2026-02-06".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_date.items.len(), 4);
    assert_eq!(by_date.total, 4);
}

#[test]
fn list_transactions_deterministic_order_by_id_when_same_timestamp() {
    let conn = setup();
    insert_account(&conn, "acc-same", "现金", "cash", "CNY");

    let mut ids = Vec::new();
    for i in 1..=5 {
        let id = insert_transaction(
            &conn,
            make_input("acc-same", "expense", i * 100, "2026-03-01"),
        )
        .unwrap();
        ids.push(id);
    }
    // 同一批导入：所有行 created_at 相同（每批一个时间戳）
    set_created_at(&conn, "2026-01-01T00:00:00Z");

    // 期望顺序 = SQLite TEXT 列的 id DESC（字典序降序，确定性 tiebreaker）
    let mut expected = ids.clone();
    expected.sort_by(|a, b| b.cmp(a));

    let mut got = Vec::new();
    for page in 1..=3 {
        let result = list_transactions_internal(
            &conn,
            &TransactionListFilter {
                page: Some(page),
                page_size: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.total, 5);
        for t in result.items {
            got.push(t.id);
        }
    }
    assert_eq!(
        got, expected,
        "同日期同时间戳应按 id DESC 稳定排序，翻页无重复无遗漏"
    );
}

#[test]
fn list_transactions_default_returns_all_with_total() {
    let conn = setup();
    insert_account(&conn, "acc-all", "现金", "cash", "CNY");
    for i in 1..=5 {
        insert_transaction(
            &conn,
            make_input("acc-all", "expense", i * 100, &format!("2026-04-{:02}", i)),
        )
        .unwrap();
    }
    let result = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(result.items.len(), 5, "缺省应返回全部");
    assert_eq!(result.total, 5);
}

#[test]
fn list_transactions_limit_path_unchanged() {
    let conn = setup();
    insert_account(&conn, "acc-lim", "现金", "cash", "CNY");
    for i in 1..=5 {
        insert_transaction(
            &conn,
            make_input("acc-lim", "expense", i * 100, &format!("2026-05-{:02}", i)),
        )
        .unwrap();
    }

    let r3 = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            limit: Some(3),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r3.items.len(), 3, "limit 路径取前 N 条");
    assert_eq!(r3.total, 5);

    let r10 = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            limit: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(r10.items.len(), 5, "limit 大于总数时返回全部");
    assert_eq!(r10.total, 5);

    let both = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            limit: Some(1),
            page: Some(1),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        both.items.len(),
        2,
        "传 page_size 时分页路径生效，limit 被忽略"
    );
}

#[test]
fn list_transactions_out_of_range_page_and_empty_result() {
    let conn = setup();
    insert_account(&conn, "acc-bnd", "现金", "cash", "CNY");
    for i in 1..=3 {
        insert_transaction(
            &conn,
            make_input("acc-bnd", "expense", i * 100, &format!("2026-06-{:02}", i)),
        )
        .unwrap();
    }

    // 超范围页码：空 items，total 不变
    let far = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            page: Some(99),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(far.items.len(), 0, "超范围页码应返回空列表");
    assert_eq!(far.total, 3);

    // page=0 视为第 1 页（page 从 1 起）
    let p0 = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            page: Some(0),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(p0.items.len(), 3);
    assert_eq!(p0.total, 3);

    // 无匹配过滤：空结果 total 0
    let none = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            kind: Some("income".into()),
            page: Some(1),
            page_size: Some(10),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(none.items.len(), 0);
    assert_eq!(none.total, 0);
}

#[test]
fn list_transactions_degenerate_inputs_do_not_panic() {
    let conn = setup();
    insert_account(&conn, "acc-deg", "现金", "cash", "CNY");
    for i in 1..=5 {
        insert_transaction(
            &conn,
            make_input("acc-deg", "expense", i * 100, &format!("2026-07-{:02}", i)),
        )
        .unwrap();
    }

    // page_size=0：进入分页路径且钳制为 1 条/页（与 InstrumentListFilter 先例一致）
    let zero_ps = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            page: Some(1),
            page_size: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(zero_ps.items.len(), 1, "page_size=0 应按 1 条/页处理");
    assert_eq!(zero_ps.total, 5);

    // limit=0：沿用 SQLite 原生语义返回空（与旧实现一致）
    let zero_limit = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            limit: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(zero_limit.items.len(), 0, "limit=0 应返回空");
    assert_eq!(zero_limit.total, 5);

    // 极端 page 不应溢出 panic，返回空页且 total 正确
    let huge_page = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            page: Some(usize::MAX),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(huge_page.items.len(), 0, "极端页码应返回空");
    assert_eq!(huge_page.total, 5);
}

#[test]
fn delete_transaction_soft_deletes() {
    let conn = setup();
    insert_account(&conn, "acc-del", "现金", "cash", "CNY");

    let id =
        insert_transaction(&conn, make_input("acc-del", "income", 1000, "2026-01-01")).unwrap();
    let count_before: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_before, 1);

    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id()],
    ).unwrap();

    let count_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_after, 0);
}

#[test]
fn delete_transaction_internal_returns_not_found_for_missing_id() {
    let conn = setup();
    insert_account(&conn, "acc-missing", "现金", "cash", "CNY");

    let err = delete_transaction_internal(&conn, "不存在的id").unwrap_err();
    match err {
        AppError::NotFound(msg) => assert!(msg.contains("交易不存在")),
        other => panic!("expected NotFound, got {other:?}"),
    }
}

#[test]
fn delete_transaction_internal_returns_not_found_for_already_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-gone", "现金", "cash", "CNY");
    let id =
        insert_transaction(&conn, make_input("acc-gone", "income", 1000, "2026-01-01")).unwrap();
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![id],
    )
    .unwrap();

    let err = delete_transaction_internal(&conn, &id).unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));
}

#[test]
fn delete_transaction_internal_frees_dedup_slot_for_reimport() {
    let conn = setup();
    insert_account(&conn, "acc-reimport", "现金", "cash", "CNY");

    let input = make_input("acc-reimport", "income", 1000, "2026-07-01");
    let first = create_transactions_internal(&conn, vec![input.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();

    delete_transaction_internal(&conn, &id).unwrap();

    let second = create_transactions_internal(&conn, vec![input], true).unwrap();
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
        kind: "buy".into(),
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
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
}

#[test]
fn normalize_transaction_rejects_transfer_without_target() {
    let conn = setup();
    insert_account(&conn, "acc-norm", "现金", "cash", "CNY");
    let result = normalize_transaction(
        &conn,
        &make_input("acc-norm", "transfer", 1000, "2026-01-01"),
    );
    match result {
        Err(AppError::Invalid(msg)) => assert!(msg.contains("目标账户")),
        _ => panic!("expected Invalid error"),
    }
}

#[test]
fn normalize_transaction_income_passthrough() {
    let conn = setup();
    insert_account(&conn, "acc-norm", "现金", "cash", "CNY");
    let norm = normalize_transaction(&conn, &make_input("acc-norm", "income", 5000, "2026-01-01"))
        .unwrap();
    assert_eq!(norm.kind, "income");
    assert_eq!(norm.amount_cents, 5000);
    assert_eq!(norm.account_id, "acc-norm");
    assert_eq!(norm.amount_native_cents, 5000, "本位币与原始币种应 1:1");
}

#[test]
fn normalize_transaction_resolves_refund_fields_from_source_expense() {
    let conn = setup();
    insert_account(&conn, "acc-norm", "现金", "cash", "CNY");
    let expense_id = insert_transaction(
        &conn,
        TransactionInput {
            kind: "expense".into(),
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: "acc-norm".into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap();
    let norm = normalize_transaction(
        &conn,
        &TransactionInput {
            kind: "refund".into(),
            amount_cents: 200,
            currency_code: "CNY".into(),
            account_id: "acc-other".into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: Some(expense_id.clone()),
            note: None,
            date: "2026-01-02".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap();
    assert_eq!(norm.kind, "refund");
    assert_eq!(norm.amount_cents, 200);
    // 退款继承原支出的账户/币种，而非调用方填的字段
    assert_eq!(norm.account_id, "acc-norm");
    assert_eq!(norm.currency_code, "CNY");
    assert_eq!(
        norm.refund_of_transaction_id.as_deref(),
        Some(expense_id.as_str())
    );
}

#[test]
fn normalize_transaction_computes_buy_amount_and_native() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv", "inst-aapl");
    let norm = normalize_transaction(
        &conn,
        &make_buy_input("acc-inv", "inst-aapl", 10.0, 10000, 500),
    )
    .unwrap();
    assert_eq!(norm.kind, "buy");
    assert_eq!(norm.amount_cents, 10 * 10000 + 500);
    assert_eq!(norm.currency_code, "USD");
    assert_eq!(
        norm.amount_native_cents, norm.amount_cents,
        "买入本位币与原始币种应 1:1"
    );
    assert_eq!(norm.category_id, None);
    assert_eq!(norm.refund_of_transaction_id, None);
}

#[test]
fn normalize_transaction_rejects_buy_non_investment_account() {
    let conn = setup();
    insert_account(&conn, "acc-cash", "现金", "cash", "CNY");
    let result = normalize_transaction(&conn, &make_buy_input("acc-cash", "inst-x", 1.0, 1000, 0));
    match result {
        Err(AppError::Invalid(msg)) => assert!(msg.contains("投资账户")),
        _ => panic!("expected Invalid error"),
    }
}

#[test]
fn delete_transaction_internal_cleans_up_buy_lots() {
    use crate::commands::investment::create_buy_transaction;
    let conn = setup();
    setup_investment_account(&conn, "acc-inv", "inst-aapl");

    let buy_id = create_buy_transaction(
        &conn,
        make_buy_input("acc-inv", "inst-aapl", 10.0, 10000, 500),
    )
    .unwrap();

    let lots: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots, 1, "买入应建仓一个 lot");
    let stx: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stx, 1);

    delete_transaction_internal(&conn, &buy_id).unwrap();

    let lots_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots_after, 0, "删除买入应清理 security_lots");
    let stx_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stx_after, 0, "删除买入应清理 security_transactions");
    let deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM transactions WHERE id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1, "交易应被软删除");
}

#[test]
fn delete_transaction_internal_rejects_partially_sold_buy() {
    use crate::commands::investment::{create_buy_transaction, create_sell_transaction};
    let conn = setup();
    setup_investment_account(&conn, "acc-inv2", "inst-msft");

    let buy_id = create_buy_transaction(
        &conn,
        make_buy_input("acc-inv2", "inst-msft", 10.0, 10000, 0),
    )
    .unwrap();

    let mut sell = make_buy_input("acc-inv2", "inst-msft", 4.0, 11000, 0);
    sell.kind = "sell".into();
    sell.date = "2026-01-20".into();
    create_sell_transaction(&conn, sell).unwrap();

    let err = delete_transaction_internal(&conn, &buy_id).unwrap_err();
    match err {
        AppError::Invalid(msg) => assert!(msg.contains("部分卖出")),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn create_refund_linked_to_expense() {
    let conn = setup();
    insert_account(&conn, "acc-ref", "现金", "cash", "CNY");

    let expense_id = insert_transaction(
        &conn,
        TransactionInput {
            kind: "expense".into(),
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: "acc-ref".into(),
            date: "2026-04-01".into(),
            category_id: None,
            to_account_id: None,
            refund_of_transaction_id: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap();

    let refund_id = insert_transaction(
        &conn,
        TransactionInput {
            kind: "refund".into(),
            amount_cents: 200,
            currency_code: "CNY".into(),
            account_id: "acc-ref".into(),
            date: "2026-04-05".into(),
            refund_of_transaction_id: Some(expense_id.clone()),
            category_id: None,
            to_account_id: None,
            note: None,
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        },
    )
    .unwrap();

    let (kind, refund_of): (String, Option<String>) = conn
        .query_row(
            "SELECT kind, refund_of_transaction_id FROM transactions WHERE id=?1",
            params![refund_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, "refund");
    assert_eq!(refund_of, Some(expense_id));
}

#[test]
fn update_transaction_internal_replaces_fields_and_bumps_version() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id =
        insert_transaction(&conn, make_input("acc-upd", "expense", 500, "2026-01-01")).unwrap();

    let mut edited = make_input("acc-upd", "expense", 900, "2026-01-05");
    edited.note = Some("改后备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, "expense");
    assert_eq!(t.amount_cents, 900);
    assert_eq!(t.date, "2026-01-05");
    assert_eq!(t.note.as_deref(), Some("改后备注"));
    assert_eq!(t.version, 2, "修改后版本号应递增");
}

#[test]
fn update_transaction_internal_returns_not_found_for_missing_or_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id =
        insert_transaction(&conn, make_input("acc-upd", "expense", 500, "2026-01-01")).unwrap();

    let err = update_transaction_internal(
        &conn,
        "不存在的id",
        make_input("acc-upd", "expense", 100, "2026-01-01"),
    )
    .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![id],
    )
    .unwrap();
    let err = update_transaction_internal(
        &conn,
        &id,
        make_input("acc-upd", "expense", 100, "2026-01-01"),
    )
    .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "已软删除应视为不存在");
}

#[test]
fn update_transaction_internal_reuses_kind_validation_transfer_needs_target() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id =
        insert_transaction(&conn, make_input("acc-upd", "expense", 500, "2026-01-01")).unwrap();

    let err = update_transaction_internal(
        &conn,
        &id,
        make_input("acc-upd", "transfer", 1000, "2026-01-02"),
    )
    .unwrap_err();
    match err {
        AppError::Invalid(msg) => assert!(msg.contains("目标账户")),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn update_transaction_internal_cross_kind_expense_to_transfer() {
    let conn = setup();
    insert_account(&conn, "acc-upd-a", "A", "cash", "CNY");
    insert_account(&conn, "acc-upd-b", "B", "cash", "CNY");
    let id =
        insert_transaction(&conn, make_input("acc-upd-a", "expense", 500, "2026-01-01")).unwrap();

    let transfer = TransactionInput {
        to_account_id: Some("acc-upd-b".into()),
        ..make_input("acc-upd-a", "transfer", 1000, "2026-01-02")
    };
    update_transaction_internal(&conn, &id, transfer).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, "transfer");
    assert_eq!(t.to_account_id.as_deref(), Some("acc-upd-b"));
}

#[test]
fn update_transaction_internal_preserves_key_and_rerun_dedup() {
    let conn = setup();
    insert_account(&conn, "acc-key", "现金", "cash", "CNY");
    let mut a = make_input("acc-key", "income", 1000, "2026-01-01");
    a.idempotency_key = Some("file:1:1".into());
    let first = create_transactions_internal(&conn, vec![a.clone()], true).unwrap();
    let id = first[0].id.clone().unwrap();

    // 编辑内容（金额/备注/日期），幂等键应保持不变。
    let mut edited = make_input("acc-key", "income", 2000, "2026-01-03");
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
    let mut rerun = make_input("acc-key", "income", 3000, "2026-02-01");
    rerun.idempotency_key = Some("file:1:1".into());
    let second = create_transactions_internal(&conn, vec![rerun], true).unwrap();
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

#[test]
fn update_transaction_internal_buy_rebuilds_lot() {
    use crate::commands::investment::create_buy_transaction;
    let conn = setup();
    setup_investment_account(&conn, "acc-inv", "inst-aapl");
    let buy_id = create_buy_transaction(
        &conn,
        make_buy_input("acc-inv", "inst-aapl", 10.0, 10000, 500),
    )
    .unwrap();

    // 编辑买入：数量/单价变化，应重建 lot 与 security_transaction。
    let edited = make_buy_input("acc-inv", "inst-aapl", 5.0, 12000, 0);
    update_transaction_internal(&conn, &buy_id, edited).unwrap();

    let t = get_transaction_internal(&conn, &buy_id).unwrap();
    assert_eq!(t.kind, "buy");
    assert_eq!(t.amount_cents, 5 * 12000, "买入金额 = 数量×单价+费用");

    let (init, remaining): (f64, f64) = conn
        .query_row(
            "SELECT initial_quantity, remaining_quantity FROM security_lots \
             WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(init, 5.0, "应重建为新的持仓数量");
    assert_eq!(remaining, 5.0);
    let stx: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stx, 1, "重建后应有一条 security_transaction");
}

#[test]
fn update_transaction_internal_rejects_partially_sold_buy() {
    use crate::commands::investment::{create_buy_transaction, create_sell_transaction};
    let conn = setup();
    setup_investment_account(&conn, "acc-inv2", "inst-msft");
    let buy_id = create_buy_transaction(
        &conn,
        make_buy_input("acc-inv2", "inst-msft", 10.0, 10000, 0),
    )
    .unwrap();

    let mut sell = make_buy_input("acc-inv2", "inst-msft", 4.0, 11000, 0);
    sell.kind = "sell".into();
    sell.date = "2026-01-20".into();
    create_sell_transaction(&conn, sell).unwrap();

    let err = update_transaction_internal(
        &conn,
        &buy_id,
        make_buy_input("acc-inv2", "inst-msft", 5.0, 10000, 0),
    )
    .unwrap_err();
    match err {
        AppError::Invalid(msg) => assert!(msg.contains("部分卖出")),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn update_transaction_internal_sell_reverses_and_reapplies() {
    use crate::commands::investment::{create_buy_transaction, create_sell_transaction};
    let conn = setup();
    setup_investment_account(&conn, "acc-inv3", "inst-tsla");
    let buy_id = create_buy_transaction(
        &conn,
        make_buy_input("acc-inv3", "inst-tsla", 10.0, 10000, 0),
    )
    .unwrap();

    let mut sell1 = make_buy_input("acc-inv3", "inst-tsla", 4.0, 11000, 0);
    sell1.kind = "sell".into();
    let sell_id = create_sell_transaction(&conn, sell1).unwrap();

    // 编辑卖出：数量 4→3、单价上涨。应先回补旧扣减再按新输入重新匹配。
    let mut sell2 = make_buy_input("acc-inv3", "inst-tsla", 3.0, 12000, 0);
    sell2.kind = "sell".into();
    sell2.date = "2026-02-01".into();
    update_transaction_internal(&conn, &sell_id, sell2).unwrap();

    let t = get_transaction_internal(&conn, &sell_id).unwrap();
    assert_eq!(t.kind, "sell");
    assert_eq!(t.amount_cents, 3 * 12000, "卖出收入 = 数量×单价");

    // 修改卖出后持仓剩余 = 10 - 3 = 7。
    let remaining: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 7.0, "修改卖出后持仓应反映新数量");

    // 旧卖出关联已清空，重建为一条新的。
    let sales: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sales, 1);
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
        make_input("acc-log-ok", "income", 1000, "2026-07-01"),
        make_input("acc-log-ok", "expense", 500, "2026-07-02"),
    ];
    let events = capture_events(|| {
        let r = create_transactions_internal(&conn, inputs, true).unwrap();
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

    let mut a = make_input("acc-log-rb", "income", 1000, "2026-07-01");
    a.idempotency_key = Some("dup-rb".into());
    let mut b = make_input("acc-log-rb", "income", 2000, "2026-07-02");
    b.idempotency_key = Some("dup-rb".into());

    let events = capture_events(|| {
        let err = create_transactions_internal(&conn, vec![a, b], false).unwrap_err();
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
        make_input("acc-log-part", "transfer", 1000, "2026-07-01"),
        make_input("acc-log-part", "income", 1000, "2026-07-02"),
    ];
    let events = capture_events(|| {
        let r = create_transactions_internal(&conn, inputs, false).unwrap();
        assert_eq!(r.len(), 2);
        assert!(!r[0].success, "转账未指定目标账户应失败");
        assert!(r[1].success);
    });

    let summary = find_batch_summary(&events).expect("应有一条批次汇总日志");
    assert_eq!(summary.level, Level::INFO);
    assert_eq!(field_value(summary, "total"), Some("2"));
    assert_eq!(field_value(summary, "failed"), Some("1"), "应含失败条数");
}
