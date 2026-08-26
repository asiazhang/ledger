use super::*;
use rusqlite::Connection;

use crate::db::{device_id, init_db, now_iso, open_in_memory};
use crate::transaction::amount::TransactionKind;
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

fn make_input(
    account_id: &str,
    kind: TransactionKind,
    amount: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        kind,
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
fn create_income_and_expense_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-crud", "现金", "cash", "CNY");

    let id1 = insert_transaction(
        &conn,
        make_input("acc-crud", TransactionKind::Income, 5000, "2026-02-01"),
    )
    .unwrap();
    let id2 = insert_transaction(
        &conn,
        TransactionInput {
            amount_cents: 1500,
            note: Some("午餐".into()),
            category_id: None,
            ..make_input("acc-crud", TransactionKind::Expense, 100, "2026-02-02")
        },
    )
    .unwrap();
    assert_ne!(id1, id2);
    let row1: (TransactionKind, String, i64, Option<String>) = conn
        .query_row(
            "SELECT kind, account_id, amount_cents, note FROM transactions WHERE id=?1",
            params![id1],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row1.0, TransactionKind::Income);
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
            kind: TransactionKind::Transfer,
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
    let (kind, from, to): (TransactionKind, String, Option<String>) = conn
        .query_row(
            "SELECT kind, account_id, to_account_id FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Transfer);
    assert_eq!(from, "acc-from");
    assert_eq!(to.as_deref(), Some("acc-to"));
}

#[test]
fn list_transactions_ordered_by_date_desc() {
    let conn = setup();
    insert_account(&conn, "acc-list", "现金", "cash", "CNY");

    insert_transaction(
        &conn,
        make_input("acc-list", TransactionKind::Income, 100, "2026-01-03"),
    )
    .unwrap();
    insert_transaction(
        &conn,
        make_input("acc-list", TransactionKind::Income, 200, "2026-01-01"),
    )
    .unwrap();
    insert_transaction(
        &conn,
        make_input("acc-list", TransactionKind::Income, 300, "2026-01-02"),
    )
    .unwrap();

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

    insert_transaction(
        &conn,
        make_input("acc-limit", TransactionKind::Income, 100, "2026-01-01"),
    )
    .unwrap();
    insert_transaction(
        &conn,
        make_input("acc-limit", TransactionKind::Income, 200, "2026-01-02"),
    )
    .unwrap();
    insert_transaction(
        &conn,
        make_input("acc-limit", TransactionKind::Income, 300, "2026-01-03"),
    )
    .unwrap();

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
            make_input(
                "acc-page",
                TransactionKind::Expense,
                i * 100,
                &format!("2026-01-{:02}", i),
            ),
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
            make_input(
                "acc-f1",
                TransactionKind::Expense,
                i * 100,
                &format!("2026-02-{:02}", i),
            ),
        )
        .unwrap();
    }
    insert_transaction(
        &conn,
        make_input("acc-f2", TransactionKind::Income, 9000, "2026-02-09"),
    )
    .unwrap();
    insert_transaction(
        &conn,
        make_input("acc-f1", TransactionKind::Income, 1000, "2026-02-10"),
    )
    .unwrap();

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
            kind: Some(TransactionKind::Income),
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
            make_input("acc-same", TransactionKind::Expense, i * 100, "2026-03-01"),
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
            make_input(
                "acc-all",
                TransactionKind::Expense,
                i * 100,
                &format!("2026-04-{:02}", i),
            ),
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
            make_input(
                "acc-lim",
                TransactionKind::Expense,
                i * 100,
                &format!("2026-05-{:02}", i),
            ),
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
            make_input(
                "acc-bnd",
                TransactionKind::Expense,
                i * 100,
                &format!("2026-06-{:02}", i),
            ),
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
            kind: Some(TransactionKind::Income),
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
            make_input(
                "acc-deg",
                TransactionKind::Expense,
                i * 100,
                &format!("2026-07-{:02}", i),
            ),
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

    let id = insert_transaction(
        &conn,
        make_input("acc-del", TransactionKind::Income, 1000, "2026-01-01"),
    )
    .unwrap();
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
    let id = insert_transaction(
        &conn,
        make_input("acc-gone", TransactionKind::Income, 1000, "2026-01-01"),
    )
    .unwrap();
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![id],
    )
    .unwrap();

    let err = delete_transaction_internal(&conn, &id).unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));
}

// ---------------------------------------------------------------------------
// issue #72：dividend / split 显式「暂不支持」拒绝
// ---------------------------------------------------------------------------

/// dividend/split 已声明但未实现：经交易创建接口显式「暂不支持」拒绝。
/// 此前经交易接口创建 dividend/split 落入 writer::normalize 的通用兜底，返回语义不明的
/// 「仅处理通用交易类型」；现改为明确的「暂不支持」——两者均不落库（见 spec #69）。
#[test]
fn insert_transaction_rejects_dividend_and_split_with_not_supported() {
    let conn = setup();
    insert_account(&conn, "acc-unsup", "现金", "cash", "CNY");

    for (kind, amount) in [(TransactionKind::Dividend, 60), (TransactionKind::Split, 0)] {
        let err = insert_transaction(&conn, make_input("acc-unsup", kind, amount, "2026-05-04"))
            .unwrap_err();
        match err {
            AppError::Invalid(msg) => assert!(
                msg.contains("暂不支持"),
                "{kind} 应报「暂不支持」，实际: {msg}"
            ),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "拒绝的交易不应落库");
}

/// 修改为 dividend/split 同样经行为层显式拒绝（单点分派覆盖创建与修改，事务回滚）。
#[test]
fn update_transaction_rejects_dividend_and_split_with_not_supported() {
    let conn = setup();
    insert_account(&conn, "acc-unsup-upd", "现金", "cash", "CNY");
    let id = insert_transaction(
        &conn,
        make_input("acc-unsup-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();

    for (kind, amount) in [(TransactionKind::Dividend, 60), (TransactionKind::Split, 0)] {
        let err = update_transaction_internal(
            &conn,
            &id,
            make_input("acc-unsup-upd", kind, amount, "2026-05-04"),
        )
        .unwrap_err();
        match err {
            AppError::Invalid(msg) => assert!(
                msg.contains("暂不支持"),
                "{kind} 应报「暂不支持」，实际: {msg}"
            ),
            other => panic!("expected Invalid, got {other:?}"),
        }
        // 修改被拒绝后原交易保持不变（事务回滚）。
        let t = get_transaction_internal(&conn, &id).unwrap();
        assert_eq!(t.kind, TransactionKind::Expense);
        assert_eq!(t.amount_cents, 500);
    }
}

/// 跨 kind 修改经行为层原子清理并重建副作用（spec #69 故事 13）：
/// expense→buy 建仓、buy→expense 清理，均不留孤儿持仓关联。
#[test]
fn update_transaction_cross_kind_rebuilds_side_effects_atomically() {
    let conn = setup();
    insert_account(&conn, "acc-cash-x", "现金", "cash", "CNY");
    setup_investment_account(&conn, "acc-x", "inst-x");

    // expense → buy：应建仓 lot。
    let id = insert_transaction(
        &conn,
        make_input("acc-cash-x", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();
    update_transaction_internal(&conn, &id, make_buy_input("acc-x", "inst-x", 3.0, 1000, 0))
        .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Buy);
    let lots: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots, 1, "expense→buy 应建仓一个 lot");

    // buy → expense：应清理持仓关联，无孤儿 lot / security_transaction。
    update_transaction_internal(
        &conn,
        &id,
        make_input("acc-cash-x", TransactionKind::Expense, 700, "2026-02-01"),
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    let (lots_after, stx_after): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1), \
                    (SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1)",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(lots_after, 0, "buy→expense 应清理 security_lots");
    assert_eq!(stx_after, 0, "buy→expense 应清理 security_transactions");
}

fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Buy,
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
    // buy/sell 本位币折算走 Amount 接缝（issue #70）：补 1:1 汇率，非默认币种账户交易不报缺汇率。
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-fix','USD','CNY',1.0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();
}

#[test]
fn delete_transaction_internal_cleans_up_buy_lots() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv", "inst-aapl");

    let buy_id = insert_transaction(
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
    let conn = setup();
    setup_investment_account(&conn, "acc-inv2", "inst-msft");

    let buy_id = insert_transaction(
        &conn,
        make_buy_input("acc-inv2", "inst-msft", 10.0, 10000, 0),
    )
    .unwrap();

    let mut sell = make_buy_input("acc-inv2", "inst-msft", 4.0, 11000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-20".into();
    insert_transaction(&conn, sell).unwrap();

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
            kind: TransactionKind::Expense,
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
            kind: TransactionKind::Refund,
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

    let (kind, refund_of): (TransactionKind, Option<String>) = conn
        .query_row(
            "SELECT kind, refund_of_transaction_id FROM transactions WHERE id=?1",
            params![refund_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, TransactionKind::Refund);
    assert_eq!(refund_of, Some(expense_id));
}

#[test]
fn update_transaction_internal_replaces_fields_and_bumps_version() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id = insert_transaction(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();

    let mut edited = make_input("acc-upd", TransactionKind::Expense, 900, "2026-01-05");
    edited.note = Some("改后备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Expense);
    assert_eq!(t.amount_cents, 900);
    assert_eq!(t.date, "2026-01-05");
    assert_eq!(t.note.as_deref(), Some("改后备注"));
    assert_eq!(t.version, 2, "修改后版本号应递增");
}

#[test]
fn update_transaction_internal_returns_not_found_for_missing_or_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id = insert_transaction(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();

    let err = update_transaction_internal(
        &conn,
        "不存在的id",
        make_input("acc-upd", TransactionKind::Expense, 100, "2026-01-01"),
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
        make_input("acc-upd", TransactionKind::Expense, 100, "2026-01-01"),
    )
    .unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)), "已软删除应视为不存在");
}

#[test]
fn update_transaction_internal_reuses_kind_validation_transfer_needs_target() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id = insert_transaction(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();

    let err = update_transaction_internal(
        &conn,
        &id,
        make_input("acc-upd", TransactionKind::Transfer, 1000, "2026-01-02"),
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
    let id = insert_transaction(
        &conn,
        make_input("acc-upd-a", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();

    let transfer = TransactionInput {
        to_account_id: Some("acc-upd-b".into()),
        ..make_input("acc-upd-a", TransactionKind::Transfer, 1000, "2026-01-02")
    };
    update_transaction_internal(&conn, &id, transfer).unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Transfer);
    assert_eq!(t.to_account_id.as_deref(), Some("acc-upd-b"));
}

#[test]
fn update_transaction_internal_buy_rebuilds_lot() {
    let conn = setup();
    setup_investment_account(&conn, "acc-inv", "inst-aapl");
    let buy_id = insert_transaction(
        &conn,
        make_buy_input("acc-inv", "inst-aapl", 10.0, 10000, 500),
    )
    .unwrap();

    // 编辑买入：数量/单价变化，应重建 lot 与 security_transaction。
    let edited = make_buy_input("acc-inv", "inst-aapl", 5.0, 12000, 0);
    update_transaction_internal(&conn, &buy_id, edited).unwrap();

    let t = get_transaction_internal(&conn, &buy_id).unwrap();
    assert_eq!(t.kind, TransactionKind::Buy);
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
    let conn = setup();
    setup_investment_account(&conn, "acc-inv2", "inst-msft");
    let buy_id = insert_transaction(
        &conn,
        make_buy_input("acc-inv2", "inst-msft", 10.0, 10000, 0),
    )
    .unwrap();

    let mut sell = make_buy_input("acc-inv2", "inst-msft", 4.0, 11000, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-20".into();
    insert_transaction(&conn, sell).unwrap();

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
    let conn = setup();
    setup_investment_account(&conn, "acc-inv3", "inst-tsla");
    let buy_id = insert_transaction(
        &conn,
        make_buy_input("acc-inv3", "inst-tsla", 10.0, 10000, 0),
    )
    .unwrap();

    let mut sell1 = make_buy_input("acc-inv3", "inst-tsla", 4.0, 11000, 0);
    sell1.kind = TransactionKind::Sell;
    let sell_id = insert_transaction(&conn, sell1).unwrap();

    // 编辑卖出：数量 4→3、单价上涨。应先回补旧扣减再按新输入重新匹配。
    let mut sell2 = make_buy_input("acc-inv3", "inst-tsla", 3.0, 12000, 0);
    sell2.kind = TransactionKind::Sell;
    sell2.date = "2026-02-01".into();
    update_transaction_internal(&conn, &sell_id, sell2).unwrap();

    let t = get_transaction_internal(&conn, &sell_id).unwrap();
    assert_eq!(t.kind, TransactionKind::Sell);
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
// issue #60：创建/修改/买入卖出行统一经 Writer 落库
// ---------------------------------------------------------------------------

/// 全部创建路径（通用 kind + buy/sell）落库行带 Writer 统一生成的审计字段：
/// version=1 / is_deleted=0 / created_at==updated_at / device_id 一致——证明
/// create 路径不再散落手写 INSERT（issue #60 验收：审计字段统一生成）。
#[test]
fn insert_transaction_audit_fields_uniform_across_kinds() {
    let conn = setup();
    insert_account(&conn, "acc-w", "现金", "cash", "CNY");
    insert_account(&conn, "acc-w2", "银行", "bank", "CNY");
    setup_investment_account(&conn, "acc-inv-w", "inst-w");

    let expense_id = insert_transaction(
        &conn,
        make_input("acc-w", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();
    let income_id = insert_transaction(
        &conn,
        make_input("acc-w", TransactionKind::Income, 900, "2026-01-02"),
    )
    .unwrap();
    let mut transfer = make_input("acc-w", TransactionKind::Transfer, 300, "2026-01-03");
    transfer.to_account_id = Some("acc-w2".into());
    let transfer_id = insert_transaction(&conn, transfer).unwrap();
    let refund_id = insert_transaction(
        &conn,
        TransactionInput {
            kind: TransactionKind::Refund,
            amount_cents: 200,
            refund_of_transaction_id: Some(expense_id.clone()),
            ..make_input("acc-w", TransactionKind::Refund, 100, "2026-01-04")
        },
    )
    .unwrap();
    let buy_id =
        insert_transaction(&conn, make_buy_input("acc-inv-w", "inst-w", 2.0, 1000, 0)).unwrap();
    let mut sell = make_buy_input("acc-inv-w", "inst-w", 1.0, 1100, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-11".into();
    let sell_id = insert_transaction(&conn, sell).unwrap();

    for id in [
        expense_id,
        income_id,
        transfer_id,
        refund_id,
        buy_id,
        sell_id,
    ] {
        let (created_at, updated_at, version, device_id, is_deleted): (
            String,
            String,
            i64,
            String,
            i64,
        ) = conn
            .query_row(
                "SELECT created_at,updated_at,version,device_id,is_deleted \
                 FROM transactions WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            created_at, updated_at,
            "新建行 created_at 与 updated_at 应一致"
        );
        assert_eq!(version, 1, "新建行 version 应为 1");
        assert_eq!(
            device_id,
            crate::db::device_id(),
            "device_id 由 Writer 统一生成"
        );
        assert_eq!(is_deleted, 0, "新建行不应被删除");
    }
}

/// 修改路径经 writer::update_row：保留 created_at、version 递增、updated_at 刷新
/// （issue #60 验收：update 不再走命令层手写 UPDATE）。
#[test]
fn update_transaction_internal_preserves_created_at_and_refreshes_audit() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id = insert_transaction(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();
    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();

    let mut edited = make_input("acc-upd", TransactionKind::Expense, 900, "2026-01-05");
    edited.note = Some("改后备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let (created_at_after, updated_at, version): (String, String, i64) = conn
        .query_row(
            "SELECT created_at,updated_at,version FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(created_at_after, created_at, "修改应保留 created_at");
    assert!(!updated_at.is_empty(), "修改应刷新 updated_at");
    assert_eq!(version, 2, "修改后 version 应递增");
}

/// 通用 kind 的本位币折算改经 Amount 接缝（基准为全局默认币种，issue #60 / spec #52）：
/// USD 账户 + USD 金额按汇率折算到 CNY，而非按账户币种 1:1 落库。
#[test]
fn insert_transaction_generic_converts_native_via_amount_seam() {
    let conn = setup();
    insert_account(&conn, "acc-usd", "美元", "cash", "USD");
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-w','USD','CNY',7.2,'2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();

    let id = insert_transaction(
        &conn,
        TransactionInput {
            currency_code: "USD".into(),
            ..make_input("acc-usd", TransactionKind::Expense, 10000, "2026-01-01")
        },
    )
    .unwrap();
    let (amount_native_cents, currency_code): (i64, String) = conn
        .query_row(
            "SELECT amount_native_cents, currency_code FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(currency_code, "USD", "原始币种保留");
    assert_eq!(
        amount_native_cents, 72000,
        "本位币金额应经 Amount 接缝折算到全局默认币种"
    );
}
