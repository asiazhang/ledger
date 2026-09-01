//! 交易查询：列表排序、过滤（账户 / kind / 日期）、分页与退化输入边界。

use super::super::*;
use super::common::{insert_account, make_input, setup};
use rusqlite::Connection;

use crate::transaction::amount::TransactionKind;
use rusqlite::params;

#[test]
fn list_transactions_ordered_by_date_desc() {
    let conn = setup();
    insert_account(&conn, "acc-list", "现金", "cash", "CNY");

    create_transaction_internal(
        &conn,
        make_input("acc-list", TransactionKind::Income, 100, "2026-01-03"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_input("acc-list", TransactionKind::Income, 200, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
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

    create_transaction_internal(
        &conn,
        make_input("acc-limit", TransactionKind::Income, 100, "2026-01-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_input("acc-limit", TransactionKind::Income, 200, "2026-01-02"),
    )
    .unwrap();
    create_transaction_internal(
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
        create_transaction_internal(
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
        create_transaction_internal(
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
    create_transaction_internal(
        &conn,
        make_input("acc-f2", TransactionKind::Income, 9000, "2026-02-09"),
    )
    .unwrap();
    create_transaction_internal(
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
fn list_transactions_involving_account_filter() {
    let conn = setup();
    insert_account(&conn, "acc-inv-1", "现金", "cash", "CNY");
    insert_account(&conn, "acc-inv-2", "银行", "bank", "CNY");
    insert_account(&conn, "acc-inv-3", "支付宝", "cash", "CNY");

    // 普通交易：现金支出（account_id 命中）
    create_transaction_internal(
        &conn,
        make_input("acc-inv-1", TransactionKind::Expense, 100, "2026-03-01"),
    )
    .unwrap();
    // 转出：现金 → 银行（account_id 命中）
    create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Transfer,
            amount_cents: 3000,
            account_id: "acc-inv-1".into(),
            to_account_id: Some("acc-inv-2".into()),
            date: "2026-03-02".into(),
            ..make_input("acc-inv-1", TransactionKind::Expense, 1, "2026-03-02")
        },
    )
    .unwrap();
    // 转入：银行 → 现金（to_account_id 命中）
    create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Transfer,
            amount_cents: 500,
            account_id: "acc-inv-2".into(),
            to_account_id: Some("acc-inv-1".into()),
            date: "2026-03-03".into(),
            ..make_input("acc-inv-1", TransactionKind::Expense, 1, "2026-03-03")
        },
    )
    .unwrap();
    // 无关账户：支付宝支出（不命中）
    create_transaction_internal(
        &conn,
        make_input("acc-inv-3", TransactionKind::Expense, 700, "2026-03-04"),
    )
    .unwrap();

    // 涉及现金：普通支出 + 转出 + 转入 = 3 条
    let involving = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        involving.items.len(),
        3,
        "涉及账户应命中普通交易与转账两侧（转出 + 转入）"
    );
    assert_eq!(involving.total, 3, "total 应为涉及账户过滤后总数");

    // 无关账户：只有自身交易，不含现金侧交易
    let unrelated = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-3".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(unrelated.total, 1, "无关账户不应命中其他账户交易");
    assert_eq!(unrelated.items[0].account_id, "acc-inv-3");

    // 与 kind 组合：涉及现金 + 仅 transfer = 2 条（转出 + 转入）
    let kind_combo = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-1".into()),
            kind: Some(TransactionKind::Transfer),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(kind_combo.total, 2, "涉及账户与类型过滤应 AND 组合");

    // 与日期组合：涉及现金 + 日期区间 [03-02, 03-03] = 2 条（转出 + 转入）
    let date_combo = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-1".into()),
            from: Some("2026-03-02".into()),
            to: Some("2026-03-03".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(date_combo.total, 2, "涉及账户与日期过滤应 AND 组合");

    // 与分页组合：涉及现金 page_size=2 → 当前页 2 条，total 仍为过滤后总数
    let paged = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-1".into()),
            page: Some(1),
            page_size: Some(2),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(paged.items.len(), 2);
    assert_eq!(paged.total, 3, "分页时 total 恒为过滤后总数");

    // 已发布字段 account_id（仅转出账户）语义不变：
    // 现金 = 普通支出 + 转出（account_id 侧），不含转入（银行 → 现金）。
    let legacy = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            account_id: Some("acc-inv-1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        legacy.total, 2,
        "account_id 仅按转出账户过滤，语义不变（不命中转入侧）"
    );
}

#[test]
fn list_transactions_deterministic_order_by_id_when_same_timestamp() {
    let conn = setup();
    insert_account(&conn, "acc-same", "现金", "cash", "CNY");

    let mut ids = Vec::new();
    for i in 1..=5 {
        let id = create_transaction_internal(
            &conn,
            make_input("acc-same", TransactionKind::Expense, i * 100, "2026-03-01"),
        )
        .unwrap()
        .id;
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
        create_transaction_internal(
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
        create_transaction_internal(
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
        create_transaction_internal(
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
        create_transaction_internal(
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
