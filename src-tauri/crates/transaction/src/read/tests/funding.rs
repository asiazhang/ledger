use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::TransactionListFilter;
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

fn seed_combined_payment(conn: &rusqlite::Connection) -> String {
    test_support::seed_account(conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(conn, "acc-balance", "余额", "cash", "CNY", 0);
    create_transaction_internal(
        conn,
        TransactionInput {
            account_id: None,
            funding: vec![
                item("acc-wallet", 2853, Some("定金")),
                item("acc-balance", 25307, None),
            ],
            ..crate::tests::common::make_input("", TransactionKind::Expense, 28160, "2026-01-01")
        },
    )
    .unwrap()
    .id
}

#[test]
fn list_and_detail_attach_fundings_in_sort_order() {
    let conn = test_support::open();
    let id = seed_combined_payment(&conn);

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(list.total, 1);
    let row = &list.items[0];
    assert_eq!(row.id, id);
    assert_eq!(row.account_id, None, "分解行主表账户列读回 null");
    assert_eq!(row.fundings.len(), 2);
    assert_eq!(row.fundings[0].account_id, "acc-wallet");
    assert_eq!(row.fundings[0].amount_cents, 2853);
    assert_eq!(row.fundings[0].label.as_deref(), Some("定金"));
    assert_eq!(row.fundings[1].account_id, "acc-balance");
    assert_eq!(row.fundings[1].amount_cents, 25307);
    assert!(!row.fundings[0].derived, "落库条目 derived 恒 false");

    let detail = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(detail.account_id, None);
    assert_eq!(detail.fundings.len(), 2);
    assert_eq!(detail.fundings[0].account_id, "acc-wallet");
}

#[test]
fn single_funding_rows_read_back_with_empty_fundings() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "现金", "cash", "CNY", 0);
    create_transaction_internal(
        &conn,
        crate::tests::common::make_input("acc", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap();

    let list = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(
        list.items[0].account_id.as_deref(),
        Some("acc"),
        "单出资行读回形态与现状兼容"
    );
    assert!(
        list.items[0].fundings.is_empty(),
        "无分解行的 fundings 恒空"
    );
}

#[test]
fn refund_default_derivation_splits_by_ratio_with_remainder_to_first() {
    let conn = test_support::open();
    let expense_id = seed_combined_payment(&conn);

    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            account_id: None,
            refund_of_transaction_id: Some(expense_id.clone()),
            ..crate::tests::common::make_input("acc", TransactionKind::Refund, 14080, "2026-01-02")
        },
    )
    .unwrap()
    .id;

    let refund = get_transaction_internal(&conn, &refund_id).unwrap();
    assert_eq!(
        refund.account_id, None,
        "原支出为分解行：退款无单一账户可继承"
    );
    assert_eq!(refund.fundings.len(), 2, "缺省按出资比例派生");
    assert!(
        refund.fundings.iter().all(|f| f.derived),
        "派生条目带 derived 标记"
    );
    assert_eq!(refund.fundings[0].account_id, "acc-wallet");
    assert_eq!(refund.fundings[0].amount_cents, 1427, "floor 1426 + 尾差 1");
    assert_eq!(refund.fundings[1].account_id, "acc-balance");
    assert_eq!(refund.fundings[1].amount_cents, 12653);
    let sum: i64 = refund.fundings.iter().map(|f| f.amount_cents).sum();
    assert_eq!(sum, 14080);
}

#[test]
fn refund_explicit_override_reads_back_persisted_breakdown() {
    let conn = test_support::open();
    let expense_id = seed_combined_payment(&conn);
    test_support::seed_account(&conn, "acc-x", "精准回退", "cash", "CNY", 0);

    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            account_id: None,
            refund_of_transaction_id: Some(expense_id),
            funding: vec![item("acc-x", 5000, None)],
            ..crate::tests::common::make_input("acc", TransactionKind::Refund, 5000, "2026-01-02")
        },
    )
    .unwrap()
    .id;

    let refund = get_transaction_internal(&conn, &refund_id).unwrap();
    assert_eq!(refund.account_id, None);
    assert_eq!(refund.fundings.len(), 1, "显式覆盖落库、不派生");
    assert!(!refund.fundings[0].derived);
    assert_eq!(refund.fundings[0].account_id, "acc-x");
}

#[test]
fn involving_account_filter_hits_funding_item_accounts() {
    let conn = test_support::open();
    seed_combined_payment(&conn);

    for acc in ["acc-wallet", "acc-balance"] {
        let filter = TransactionListFilter {
            involving_account_id: Some(acc.into()),
            ..TransactionListFilter::default()
        };
        let hit = list_transactions_internal(&conn, &filter).unwrap();
        assert_eq!(hit.total, 1, "按出资方 {acc} 过滤应命中组合支付行");
        assert_eq!(hit.items[0].fundings.len(), 2);
    }
}

#[test]
fn delete_funding_row_removes_it_from_involving_filter() {
    let conn = test_support::open();
    let id = seed_combined_payment(&conn);
    delete_transaction_internal(&conn, &id).unwrap();

    let filter = TransactionListFilter {
        involving_account_id: Some("acc-wallet".into()),
        ..TransactionListFilter::default()
    };
    let hit = list_transactions_internal(&conn, &filter).unwrap();
    assert_eq!(hit.total, 0, "软删行不参与涉及账户过滤（子行随主行失效）");
}

#[test]
fn search_results_carry_funding_breakdown() {
    let conn = test_support::open();
    let id = seed_combined_payment(&conn);

    let hit = search_transactions_internal(&conn, "", 1, 50, Some(28160), Some(28160), None, None)
        .unwrap();
    assert_eq!(hit.total, 1, "金额筛选应命中组合支付行");
    let row = &hit.items[0];
    assert_eq!(row.id, id);
    assert_eq!(row.account_id, None, "分解行主表账户列读回 null");
    assert_eq!(row.fundings.len(), 2, "搜索页分解行同样携带出资项");
    assert_eq!(row.fundings[0].account_id, "acc-wallet");
    assert_eq!(row.fundings[1].amount_cents, 25307);
}

#[test]
fn search_does_not_drop_split_rows_when_soft_deleted_accounts_exist() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-gone", "已删账户", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-balance", "余额", "cash", "CNY", 0);
    let created = create_transaction_internal(
        &conn,
        TransactionInput {
            account_id: None,
            funding: vec![
                item("acc-wallet", 2853, None),
                item("acc-balance", 25307, None),
            ],
            note: Some("组合支付订单".into()),
            ..crate::tests::common::make_input("", TransactionKind::Expense, 28160, "2026-01-01")
        },
    )
    .unwrap();

    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-gone'", [])
        .unwrap();

    let kw =
        search_transactions_internal(&conn, "组合支付", 1, 50, None, None, None, None).unwrap();
    assert_eq!(kw.total, 1, "关键字路径：软删账户在场不滤出分解行");
    assert_eq!(kw.items[0].id, created.id);

    let filtered =
        search_transactions_internal(&conn, "", 1, 50, Some(28160), Some(28160), None, None)
            .unwrap();
    assert_eq!(filtered.total, 1, "仅筛选路径：软删账户在场不滤出分解行");
}

#[test]
fn detail_main_row_and_fundings_share_one_snapshot() {
    let dir = ScratchDir::new("tx-detail-read-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_combined_payment(&conn);

    let baseline = get_transaction_internal(&conn, &id).unwrap();

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transaction_fundings WHERE transaction_id IN",
        &[&format!(
            "UPDATE transaction_fundings SET amount_cents = amount_cents + 7 \
             WHERE transaction_id = '{id}' AND sort = 0"
        )],
    );
    let after = get_transaction_internal(&conn, &id).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中详情读闭包（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        baseline.fundings, after.fundings,
        "详情主行与出资子行必须同快照——注入写要么整体进快照、要么整体不进"
    );
}

#[test]
fn list_page_rows_and_fundings_share_one_snapshot() {
    let dir = ScratchDir::new("tx-list-read-snapshot");
    let conn = test_support::open_file(dir.path());
    let id = seed_combined_payment(&conn);

    let baseline = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM transaction_fundings WHERE transaction_id IN",
        &[&format!(
            "UPDATE transaction_fundings SET amount_cents = amount_cents + 7 \
             WHERE transaction_id = '{id}' AND sort = 0"
        )],
    );
    let after = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中列表读闭包（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );
    assert_eq!(
        baseline.items[0].fundings, after.items[0].fundings,
        "列表页行与出资子行必须同快照——注入写要么整体进快照、要么整体不进"
    );
}
