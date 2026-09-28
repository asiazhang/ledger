//! 订单汇总读回与读闭包（issue #1862 / ADR-0138 决策 9）：同单行集口径（软删
//! 排除、日期升序、空单零行）、行数/合计/币种与出资构成聚合（分解行逐出资项、
//! 单出资行按主账户整额、同账户合并）、订单汇总读快照探针。
//!
//! 全部经根包实例驱动（行为路径测试纪律，见 crate 根 lib.rs 头注）。

use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::ScratchDir;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};

/// 分解条目构造器。
fn item(account_id: &str, amount_cents: i64, label: Option<&str>) -> TransactionFundingInput {
    TransactionFundingInput {
        account_id: account_id.into(),
        amount_cents,
        label: label.map(String::from),
    }
}

/// 带来源订单号的输入构造器（其余字段与 [`crate::tests::common::make_input`] 同款）。
fn order_input(
    account_id: &str,
    kind: TransactionKind,
    amount: i64,
    date: &str,
    order_no: &str,
) -> TransactionInput {
    TransactionInput {
        source_order_no: Some(order_no.into()),
        ..crate::tests::common::make_input(account_id, kind, amount, date)
    }
}

/// 组合支付订单（ADR-0138 背景：订单 `JD-9001` ¥281.60 = 小金库 ¥28.53 + 余额
/// ¥253.07）+ 一行单出资商品行：多账户分解与单出资行混在同一订单。
fn seed_order(conn: &rusqlite::Connection) -> (String, String) {
    test_support::seed_account(conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(conn, "acc-balance", "余额", "cash", "CNY", 0);
    let decomposed = create_transaction_internal(
        conn,
        TransactionInput {
            account_id: None,
            funding: vec![
                item("acc-wallet", 2853, Some("定金")),
                item("acc-balance", 25307, None),
            ],
            ..order_input("", TransactionKind::Expense, 28160, "2026-01-01", "JD-9001")
        },
    )
    .unwrap()
    .id;
    let plain = create_transaction_internal(
        conn,
        order_input(
            "acc-wallet",
            TransactionKind::Expense,
            1000,
            "2026-01-02",
            "JD-9001",
        ),
    )
    .unwrap()
    .id;
    (decomposed, plain)
}

// ---------------------------------------------------------------------------
// 同单行集与聚合口径
// ---------------------------------------------------------------------------

#[test]
fn order_summary_aggregates_rows_fundings_and_excludes_soft_deleted() {
    let conn = test_support::open();
    let (decomposed, plain) = seed_order(&conn);

    let summary = get_transaction_order_summary(&conn, "JD-9001").unwrap();
    assert_eq!(summary.source_order_no, "JD-9001");
    // 行集：同单两行、软删排除、日期升序（对照回单的阅读序）。
    assert_eq!(summary.row_count, 2);
    assert_eq!(
        summary
            .items
            .iter()
            .map(|t| t.id.as_str())
            .collect::<Vec<_>>(),
        vec![decomposed.as_str(), plain.as_str()],
        "行序为日期升序（回单阅读序）"
    );
    // 明细行的 attach 契约与列表同形：分解行随行携带出资项。
    assert_eq!(summary.items[0].fundings.len(), 2);
    assert_eq!(summary.items[0].fundings[0].account_id, "acc-wallet");
    assert!(summary.items[1].fundings.is_empty());

    // 合计与币种：原始币种直和、唯一币种携带。
    assert_eq!(summary.total_amount_cents, 28160 + 1000);
    assert_eq!(summary.currency_code.as_deref(), Some("CNY"));

    // 出资构成：分解行逐出资项 + 单出资行按主账户整额，同账户合并——
    // 小金库 = 2853（定金）+ 1000（单出资行）= 3853；余额 = 25307。
    assert_eq!(summary.accounts.len(), 2);
    assert_eq!(summary.accounts[0].account_id, "acc-wallet");
    assert_eq!(summary.accounts[0].amount_cents, 2853 + 1000);
    assert_eq!(summary.accounts[1].account_id, "acc-balance");
    assert_eq!(summary.accounts[1].amount_cents, 25307);
    // 口径自洽：出资构成之和 == 合计。
    let contribution_sum: i64 = summary.accounts.iter().map(|a| a.amount_cents).sum();
    assert_eq!(contribution_sum, summary.total_amount_cents);
}

#[test]
fn order_summary_single_account_order_has_one_contribution_and_mixed_currency_none() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "现金A", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "现金B", "cash", "USD", 0);
    // USD 行折算需要交易周的 USD→CNY 周点（Amount 接缝按 ISO 命中）。
    // 周点落在交易所属 ISO 周（2026-02-02 起的一周，2026-02-01 属上一周）。
    test_support::seed_fx_rate_history(&conn, "fx-usd", "USD", "CNY", "2026-02-02", 7.0);
    create_transaction_internal(
        &conn,
        order_input(
            "acc-a",
            TransactionKind::Expense,
            500,
            "2026-02-01",
            "JD-9002",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        order_input(
            "acc-a",
            TransactionKind::Income,
            200,
            "2026-02-02",
            "JD-9002",
        ),
    )
    .unwrap();
    // 混合币种行（理论边缘）：币种唯一性破缺时不静默混算。
    create_transaction_internal(
        &conn,
        TransactionInput {
            currency_code: "USD".into(),
            ..order_input(
                "acc-b",
                TransactionKind::Expense,
                300,
                "2026-02-03",
                "JD-9002",
            )
        },
    )
    .unwrap();

    let summary = get_transaction_order_summary(&conn, "JD-9002").unwrap();
    assert_eq!(summary.row_count, 3);
    assert_eq!(
        summary.currency_code, None,
        "混合币种不携带币种（不静默混算）"
    );
    // 出资构成：两行同账户合并为一条 + 混合币种行账户一条。
    assert_eq!(summary.accounts.len(), 2);
    assert_eq!(summary.accounts[0].account_id, "acc-a");
    assert_eq!(summary.accounts[0].amount_cents, 500 + 200);
    assert_eq!(summary.accounts[1].account_id, "acc-b");
    assert_eq!(summary.accounts[1].amount_cents, 300);
}

#[test]
fn order_summary_empty_order_returns_zero_rows_and_other_orders_isolated() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "现金", "cash", "CNY", 0);
    let (decomposed, plain) = seed_order(&conn);

    // 无该订单号的行：不命中（手动行缺省 None）。
    let manual = get_transaction_order_summary(&conn, "NO-SUCH-ORDER").unwrap();
    assert_eq!(manual.row_count, 0);
    assert!(manual.items.is_empty());
    assert!(manual.accounts.is_empty());

    // 软删一行后行集与聚合同步收缩（行集与聚合同口径，同单行数 2 → 1）。
    delete_transaction_internal(&conn, &decomposed).unwrap();
    let summary = get_transaction_order_summary(&conn, "JD-9001").unwrap();
    assert_eq!(summary.row_count, 1);
    assert_eq!(summary.items[0].id, plain);
    assert_eq!(summary.total_amount_cents, 1000);
    assert_eq!(summary.accounts.len(), 1);
    assert_eq!(summary.accounts[0].amount_cents, 1000);
}

#[test]
fn refund_row_keeps_caller_provided_order_no_without_inheritance() {
    let conn = test_support::open();
    let (expense, _) = seed_order(&conn);

    // refund 不继承来源订单号（继承闭集只有账户/币种/分类/商户）：
    // 缺省 None；调用方显式携带（如退款单号）按给什么落什么。
    let refund = create_transaction_internal(
        &conn,
        TransactionInput {
            refund_of_transaction_id: Some(expense.clone()),
            source_order_no: Some("JD-REFUND-77".into()),
            ..crate::tests::common::make_input(
                "acc-wallet",
                TransactionKind::Refund,
                500,
                "2026-01-03",
            )
        },
    )
    .unwrap()
    .id;
    let row = get_transaction_internal(&conn, &refund).unwrap();
    assert_eq!(row.source_order_no.as_deref(), Some("JD-REFUND-77"));

    // 订单汇总按退款单号自成一行集（不与原订单混单）。
    let refund_order = get_transaction_order_summary(&conn, "JD-REFUND-77").unwrap();
    assert_eq!(refund_order.row_count, 1);
    assert_eq!(refund_order.items[0].id, refund);
}

// ---------------------------------------------------------------------------
// 写路径往返（列落库 + 修改全量语义）
// ---------------------------------------------------------------------------

#[test]
fn source_order_no_roundtrips_create_and_update() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        order_input(
            "acc",
            TransactionKind::Expense,
            1000,
            "2026-01-01",
            "JD-100",
        ),
    )
    .unwrap()
    .id;
    assert_eq!(
        get_transaction_internal(&conn, &id)
            .unwrap()
            .source_order_no
            .as_deref(),
        Some("JD-100"),
        "创建路径来源订单号落库并读回"
    );

    // 修改全量语义：改为新单号。
    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            source_order_no: Some("JD-200".into()),
            ..order_input(
                "acc",
                TransactionKind::Expense,
                1000,
                "2026-01-01",
                "JD-100",
            )
        },
    )
    .unwrap();
    assert_eq!(
        get_transaction_internal(&conn, &id)
            .unwrap()
            .source_order_no
            .as_deref(),
        Some("JD-200")
    );

    // 修改全量语义：缺省（None）即清除。
    update_transaction_internal(
        &conn,
        &id,
        crate::tests::common::make_input("acc", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap();
    assert_eq!(
        get_transaction_internal(&conn, &id)
            .unwrap()
            .source_order_no,
        None,
        "修改缺省即清除（全字段替换）"
    );
}

#[test]
fn list_and_search_rows_carry_source_order_no() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            note: Some("带备注行".into()),
            ..order_input(
                "acc",
                TransactionKind::Expense,
                1000,
                "2026-01-01",
                "JD-300",
            )
        },
    )
    .unwrap()
    .id;

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(list.items[0].source_order_no.as_deref(), Some("JD-300"));
    let detail = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(detail.source_order_no.as_deref(), Some("JD-300"));
    let search =
        search_transactions_internal(&conn, "带备注", 1, 10, None, None, None, None).unwrap();
    assert_eq!(search.items[0].source_order_no.as_deref(), Some("JD-300"));
}

// ---------------------------------------------------------------------------
// 读快照探针（删除接线即红）
// ---------------------------------------------------------------------------

/// 订单汇总读闭包（行集 SELECT ↔ 出资子行 SELECT）必须同快照：探针在行集查询
/// 开始前于另一连接给同单一行追加出资子行——
/// - 读闭包无快照保护（红）：行集读旧（合计 29160）、出资读新（构成之和 29161），
///   「出资构成之和 == 合计」的自洽口径变红；
/// - 读闭包收进读事务（绿）：注入写被挡住，两读同见一套数，断言绿。
#[test]
fn order_summary_rows_and_fundings_share_one_snapshot() {
    let dir = ScratchDir::new("tx-order-read-snapshot");
    let conn = test_support::open_file(dir.path());
    let (decomposed, _) = seed_order(&conn);

    // 探针：出资子行查询（FROM transaction_fundings，行集首条语句不含该子串）
    // 开始前，另一连接提交一笔出资子行追加写——注入落在行集与出资两语句之间。
    test_support::seed_account(&conn, "acc-extra", "追加账户", "cash", "CNY", 0);
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transaction_fundings",
        &[&format!(
            "INSERT INTO transaction_fundings(transaction_id, sort, account_id, amount_cents) \
             VALUES ('{decomposed}', 99, 'acc-extra', 1)"
        )],
    );

    let summary = get_transaction_order_summary(&conn, "JD-9001").unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中行集查询（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert!(
        !summary.items.is_empty(),
        "种子应产出同单行（空行集会让口径断言空转）"
    );
    let contribution_sum: i64 = summary.accounts.iter().map(|a| a.amount_cents).sum();
    assert_eq!(
        contribution_sum, summary.total_amount_cents,
        "出资构成之和必须与行集合计同快照：行集之后、出资之前落了写提交即口径错位"
    );
}
