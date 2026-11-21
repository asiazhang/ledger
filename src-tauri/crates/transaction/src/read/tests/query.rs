use crate::tests::common::{make_buy_input, make_input};
use ledger_categories::{CategoryInput, create_category};
use rusqlite::Connection;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::ledger_transaction::{TransactionInput, TransactionListFilter};
use tauri_app_lib::test_support;

use rusqlite::params;
use tauri_app_lib::ledger_transaction::amount::{FxRateSource, TransactionKind};

#[test]
fn list_transactions_ordered_by_date_desc() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-list", "现金", "cash", "CNY", 0);

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
    assert_eq!(rows[0].1, 100);
    assert_eq!(rows[1].1, 300);
    assert_eq!(rows[2].1, 200);
}

#[test]
fn list_transactions_limit() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-limit", "现金", "cash", "CNY", 0);

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

fn set_created_at(conn: &Connection, created_at: &str) {
    conn.execute(
        "UPDATE transactions SET created_at=?1, updated_at=?1",
        params![created_at],
    )
    .unwrap();
}

#[test]
fn list_transactions_pagination_returns_page_and_total() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-page", "现金", "cash", "CNY", 0);

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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-f1", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-f2", "银行", "bank", "CNY", 0);

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
            kinds: Some(vec![TransactionKind::Income]),
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-inv-1", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-inv-2", "银行", "bank", "CNY", 0);
    test_support::seed_account(&conn, "acc-inv-3", "支付宝", "cash", "CNY", 0);

    create_transaction_internal(
        &conn,
        make_input("acc-inv-1", TransactionKind::Expense, 100, "2026-03-01"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_transfer("acc-inv-1", "acc-inv-2", 3000, "2026-03-02"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_transfer("acc-inv-2", "acc-inv-1", 500, "2026-03-03"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_input("acc-inv-3", TransactionKind::Expense, 700, "2026-03-04"),
    )
    .unwrap();

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

    let unrelated = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-3".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(unrelated.total, 1, "无关账户不应命中其他账户交易");
    assert_eq!(unrelated.items[0].account_id.as_deref(), Some("acc-inv-3"));

    let kind_combo = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-1".into()),
            kinds: Some(vec![TransactionKind::Transfer]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(kind_combo.total, 2, "涉及账户与类型过滤应 AND 组合");

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
fn list_transactions_involving_account_filter_includes_funding_end() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inv-f", "inst-f");
    test_support::seed_account(&conn, "acc-fund-usd", "美元卡", "bank", "USD", 0);
    test_support::seed_account(&conn, "acc-fund-other", "无关账户", "cash", "CNY", 0);

    create_transaction_internal(
        &conn,
        TransactionInput {
            funding_account_id: Some("acc-fund-usd".into()),
            ..make_buy_input("acc-inv-f", "inst-f", 10.0, 100000, 0)
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            funding_account_id: Some("acc-fund-usd".into()),
            kind: TransactionKind::Sell,
            ..make_buy_input("acc-inv-f", "inst-f", 4.0, 110000, 0)
        },
    )
    .unwrap();

    let by_funding = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-fund-usd".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        by_funding.total, 2,
        "按出资账户过滤应命中其出资的买入与卖出"
    );
    assert!(
        by_funding
            .items
            .iter()
            .all(|t| t.funding_account_id.as_deref() == Some("acc-fund-usd")),
        "命中行的出资端均应为该账户"
    );

    let by_investment = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-inv-f".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_investment.total, 2, "投资账户端命中不变（买入 + 卖出）");

    let by_other = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            involving_account_id: Some("acc-fund-other".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_other.total, 0, "无关账户不应命中");

    let legacy = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            account_id: Some("acc-fund-usd".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        legacy.total, 0,
        "account_id 过滤不含出资端，语义不变（只增不改）"
    );
}

fn make_transfer(from: &str, to: &str, amount: i64, date: &str) -> TransactionInput {
    TransactionInput {
        kind: TransactionKind::Transfer,
        amount_cents: amount,
        account_id: Some(from.into()),
        to_account_id: Some(to.into()),
        date: date.into(),
        ..make_input(from, TransactionKind::Expense, amount, date)
    }
}

#[test]
fn list_transactions_hide_investment_related_excludes_three_ends() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-hide-bank", "银行卡", "bank", "CNY", 0);
    test_support::seed_account(&conn, "acc-hide-inv", "投资账户", "investment", "CNY", 0);
    test_support::seed_account(&conn, "acc-hide-inv2", "投资账户二", "investment", "CNY", 0);
    test_support::seed_account(&conn, "acc-hide-cash", "现金", "cash", "CNY", 0);

    create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("充值转账".into()),
            ..make_transfer("acc-hide-bank", "acc-hide-inv", 1000, "2026-04-01")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("提现".into()),
            ..make_transfer("acc-hide-inv", "acc-hide-bank", 500, "2026-04-02")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("投资账户直接付费支出".into()),
            ..make_input("acc-hide-inv", TransactionKind::Expense, 30, "2026-04-03")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("两端皆投资".into()),
            ..make_transfer("acc-hide-inv", "acc-hide-inv2", 200, "2026-04-04")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("银行卡日常支出".into()),
            ..make_input("acc-hide-bank", TransactionKind::Expense, 40, "2026-04-05")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("现金收入".into()),
            ..make_input("acc-hide-cash", TransactionKind::Income, 50, "2026-04-06")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("现金转账".into()),
            ..make_transfer("acc-hide-cash", "acc-hide-bank", 60, "2026-04-07")
        },
    )
    .unwrap();

    let all = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(all.total, 7, "缺省不携带参数应返回全部行");

    let hidden = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            hide_investment_related: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hidden.total, 3, "三端任一命中投资账户的行应被排除");
    let visible: Vec<&str> = hidden
        .items
        .iter()
        .filter_map(|t| t.note.as_deref())
        .collect();
    for scenario in ["充值转账", "提现", "投资账户直接付费支出", "两端皆投资"]
    {
        assert!(
            !visible.contains(&scenario),
            "三端命中的「{scenario}」行应被排除，实际可见: {visible:?}"
        );
    }
    for kept in ["银行卡日常支出", "现金收入", "现金转账"] {
        assert!(
            visible.contains(&kept),
            "非投资相关的「{kept}」应保留，实际可见: {visible:?}"
        );
    }
    assert_eq!(
        visible,
        vec!["现金转账", "现金收入", "银行卡日常支出"],
        "保留行按日期倒序"
    );
}

#[test]
fn list_transactions_hide_investment_related_is_account_based_and_combines() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cat-bank", "银行卡", "bank", "CNY", 0);
    test_support::seed_account(&conn, "acc-cat-inv", "投资账户", "investment", "CNY", 0);
    let fee_category_id = create_category(
        &conn,
        CategoryInput {
            name: "投资费用".into(),
            kind: "expense".into(),
            parent_id: None,
            icon: None,
        },
    )
    .unwrap();

    create_transaction_internal(
        &conn,
        TransactionInput {
            category_id: Some(fee_category_id.clone()),
            ..make_input("acc-cat-bank", TransactionKind::Expense, 100, "2026-05-01")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_input("acc-cat-inv", TransactionKind::Expense, 200, "2026-05-02"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_input("acc-cat-bank", TransactionKind::Income, 300, "2026-05-03"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_transfer("acc-cat-bank", "acc-cat-inv", 400, "2026-05-04"),
    )
    .unwrap();

    let hide = TransactionListFilter {
        hide_investment_related: Some(true),
        ..Default::default()
    };
    let hidden = list_transactions_internal(&conn, &hide).unwrap();
    assert_eq!(hidden.total, 2, "投资相关行排除后应剩两行");
    assert_eq!(
        hidden.items[0].amount_cents, 300,
        "保留行首行应为银行卡收入"
    );
    let kept_expense = &hidden.items[1];
    assert_eq!(
        kept_expense.amount_cents, 100,
        "银行卡『投资费用』分类支出应保留"
    );
    assert_eq!(
        kept_expense.category_id.as_deref(),
        Some(fee_category_id.as_str()),
        "保留的正是挂分类的那行（按账户不按分类）"
    );

    let off = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            hide_investment_related: Some(false),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(off.total, 4, "false 视为未携带，返回全量");

    let income_only = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            kinds: Some(vec![TransactionKind::Income]),
            ..hide.clone()
        },
    )
    .unwrap();
    assert_eq!(income_only.total, 1, "kinds 与偏好收窄应 AND 组合");

    let inv_account = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            account_id: Some("acc-cat-inv".into()),
            ..hide.clone()
        },
    )
    .unwrap();
    assert_eq!(inv_account.total, 0, "账户过滤与偏好收窄应 AND 组合");

    let paged = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            page: Some(2),
            page_size: Some(1),
            ..hide.clone()
        },
    )
    .unwrap();
    assert_eq!(paged.items.len(), 1, "第 2 页应返回 1 行");
    assert_eq!(paged.total, 2, "total 恒为过滤后总数");
}

#[test]
fn list_transactions_deterministic_order_by_id_when_same_timestamp() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-same", "现金", "cash", "CNY", 0);

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
    set_created_at(&conn, test_support::FIXED_NOW);

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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-all", "现金", "cash", "CNY", 0);
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-lim", "现金", "cash", "CNY", 0);
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-bnd", "现金", "cash", "CNY", 0);
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

    let none = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            kinds: Some(vec![TransactionKind::Income]),
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
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-deg", "现金", "cash", "CNY", 0);
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

fn make_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Sell,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

fn make_convert_input(
    account_id: &str,
    instrument_id: &str,
    to_instrument_id: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Convert,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-02-01".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(10.0),
        price_cents: None,
        fee_cents: Some(0),
        to_instrument_id: Some(to_instrument_id.into()),
        to_quantity: Some(10.0),
        out_amount_cents: Some(100_000),
        in_amount_cents: Some(100_000),
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

#[test]
fn list_transactions_filter_by_instrument_hits_buy_and_sell() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inst", "inst-hit");
    test_support::seed_instrument(&conn, "inst-other", "OTHER", "Other", "USD", "unknown");

    create_transaction_internal(
        &conn,
        TransactionInput {
            currency_code: "USD".into(),
            ..make_input("acc-inst", TransactionKind::Expense, 500, "2026-01-05")
        },
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_buy_input("acc-inst", "inst-hit", 10.0, 100_000, 0),
    )
    .unwrap();
    let sell_id =
        create_transaction_internal(&conn, make_sell_input("acc-inst", "inst-hit", 4.0, 150_000))
            .unwrap()
            .id;
    create_transaction_internal(
        &conn,
        make_buy_input("acc-inst", "inst-other", 10.0, 100_000, 0),
    )
    .unwrap();

    let result = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            instrument_id: Some("inst-hit".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.total, 2, "命中标的的 buy 与 sell 各一条");
    assert!(result.items.iter().any(|t| t.kind == TransactionKind::Buy));
    assert!(
        result
            .items
            .iter()
            .any(|t| t.kind == TransactionKind::Sell && t.id == sell_id)
    );
    assert!(
        !result
            .items
            .iter()
            .any(|t| t.kind == TransactionKind::Expense)
    );
}

#[test]
fn list_transactions_filter_by_instrument_matches_convert_either_leg() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-conv", "inst-out");
    test_support::seed_instrument(&conn, "inst-in", "TOIN", "To In", "USD", "unknown");

    create_transaction_internal(
        &conn,
        make_buy_input("acc-conv", "inst-out", 10.0, 100_000, 0),
    )
    .unwrap();
    let convert_id =
        create_transaction_internal(&conn, make_convert_input("acc-conv", "inst-out", "inst-in"))
            .unwrap()
            .id;

    let out = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            instrument_id: Some("inst-out".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(out.total, 2, "买入 + 转换行（转出腿）命中");
    assert!(out.items.iter().any(|t| t.id == convert_id));

    let inp = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            instrument_id: Some("inst-in".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(inp.total, 1, "convert 转入腿命中即算");
    assert_eq!(inp.items[0].id, convert_id);
}

#[test]
fn list_transactions_filter_by_instrument_combines_with_account() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-i1", "inst-x");
    test_support::seed_account(&conn, "acc-i2", "另一投资户", "investment", "USD", 0);

    create_transaction_internal(&conn, make_buy_input("acc-i1", "inst-x", 10.0, 100_000, 0))
        .unwrap();
    create_transaction_internal(&conn, make_buy_input("acc-i2", "inst-x", 10.0, 100_000, 0))
        .unwrap();

    let combined = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            account_id: Some("acc-i2".into()),
            instrument_id: Some("inst-x".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(combined.total, 1, "标的与账户维度 AND 组合");
    assert!(
        combined
            .items
            .iter()
            .all(|t| t.account_id.as_deref() == Some("acc-i2"))
    );
}

#[test]
fn list_transactions_filter_by_unknown_instrument_returns_empty() {
    let conn = test_support::open();
    test_support::seed_investment_setup(&conn, "acc-inst", "inst-hit");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-inst", "inst-hit", 10.0, 100_000, 0),
    )
    .unwrap();

    let result = list_transactions_internal(
        &conn,
        &TransactionListFilter {
            instrument_id: Some("inst-none".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.total, 0);
    assert!(result.items.is_empty());
}

#[test]
fn readback_exposes_fx_rate_trace_and_distinguishes_sources() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-fx", "港币", "cash", "HKD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-read", "HKD", "CNY", "2026-01-05", 0.9);

    let mut input = make_input("acc-fx", TransactionKind::Expense, 10000, "2026-01-07");
    input.currency_code = "HKD".into();
    let fx_id = create_transaction_internal(&conn, input).unwrap().id;
    test_support::seed_account(&conn, "acc-cny-ref", "现金", "cash", "CNY", 0);
    let cny_id = create_transaction_internal(
        &conn,
        make_input("acc-cny-ref", TransactionKind::Expense, 500, "2026-01-07"),
    )
    .unwrap()
    .id;

    let result = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    let by_id = |id: &str| {
        result
            .items
            .iter()
            .find(|t| t.id == id)
            .unwrap_or_else(|| panic!("列表应含行 {id}"))
    };
    let fx_row = by_id(&fx_id);
    assert_eq!(fx_row.amount_native_cents, 9000);
    assert_eq!(fx_row.fx_rate_used, Some(0.9));
    assert_eq!(fx_row.fx_rate_source, Some(FxRateSource::Series));
    let cny_row = by_id(&cny_id);
    assert_eq!(cny_row.fx_rate_used, None, "同币种不折算：无汇率留痕");
    assert_eq!(cny_row.fx_rate_source, None, "同币种不折算：无来源留痕");

    let got = get_transaction_internal(&conn, &fx_id).unwrap();
    assert_eq!(got.fx_rate_source, Some(FxRateSource::Series));

    conn.execute(
        "UPDATE transactions SET fx_rate_used=0.85, fx_rate_source='explicit' WHERE id=?1",
        params![fx_id],
    )
    .unwrap();
    let got = get_transaction_internal(&conn, &fx_id).unwrap();
    assert_eq!(got.fx_rate_used, Some(0.85));
    assert_eq!(got.fx_rate_source, Some(FxRateSource::Explicit));

    let found = search_transactions_internal(&conn, "港币", 1, 20, None, None, None, None).unwrap();
    assert!(
        found
            .items
            .iter()
            .any(|t| t.id == fx_id && t.fx_rate_source == Some(FxRateSource::Explicit)),
        "搜索读回应带折算来源留痕"
    );
}
