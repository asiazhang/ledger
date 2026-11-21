use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::ScratchDir;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};

fn item(account_id: &str, amount_cents: i64, label: Option<&str>) -> TransactionFundingInput {
    TransactionFundingInput {
        account_id: account_id.into(),
        amount_cents,
        label: label.map(String::from),
    }
}

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

#[test]
fn order_summary_aggregates_rows_fundings_and_excludes_soft_deleted() {
    let conn = test_support::open();
    let (decomposed, plain) = seed_order(&conn);

    let summary = get_transaction_order_summary(&conn, "JD-9001").unwrap();
    assert_eq!(summary.source_order_no, "JD-9001");
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
    assert_eq!(summary.items[0].fundings.len(), 2);
    assert_eq!(summary.items[0].fundings[0].account_id, "acc-wallet");
    assert!(summary.items[1].fundings.is_empty());

    assert_eq!(summary.total_amount_cents, 28160 + 1000);
    assert_eq!(summary.currency_code.as_deref(), Some("CNY"));

    assert_eq!(summary.accounts.len(), 2);
    assert_eq!(summary.accounts[0].account_id, "acc-wallet");
    assert_eq!(summary.accounts[0].amount_cents, 2853 + 1000);
    assert_eq!(summary.accounts[1].account_id, "acc-balance");
    assert_eq!(summary.accounts[1].amount_cents, 25307);
    let contribution_sum: i64 = summary.accounts.iter().map(|a| a.amount_cents).sum();
    assert_eq!(contribution_sum, summary.total_amount_cents);
}

#[test]
fn order_summary_single_account_order_has_one_contribution_and_mixed_currency_none() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "现金A", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "现金B", "cash", "USD", 0);
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

    let manual = get_transaction_order_summary(&conn, "NO-SUCH-ORDER").unwrap();
    assert_eq!(manual.row_count, 0);
    assert!(manual.items.is_empty());
    assert!(manual.accounts.is_empty());

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

    let refund_order = get_transaction_order_summary(&conn, "JD-REFUND-77").unwrap();
    assert_eq!(refund_order.row_count, 1);
    assert_eq!(refund_order.items[0].id, refund);
}

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

#[test]
fn order_summary_rows_and_fundings_share_one_snapshot() {
    let dir = ScratchDir::new("tx-order-read-snapshot");
    let conn = test_support::open_file(dir.path());
    let (decomposed, _) = seed_order(&conn);

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
