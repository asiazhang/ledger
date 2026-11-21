use crate::tests::common::make_input;
use tauri_app_lib::ledger_transaction::TransactionListFilter;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::test_support;
use tauri_app_lib::test_support::ScratchDir;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};

#[test]
fn list_total_and_items_share_one_snapshot() {
    let dir = ScratchDir::new("tx-list-read-snapshot");
    let conn = test_support::open_file(dir.path());
    test_support::seed_account(&conn, "acc-snap", "现金", "cash", "CNY", 0);
    let mut ids: Vec<String> = Vec::new();
    for day in 1..=3i64 {
        ids.push(
            create_transaction_internal(
                &conn,
                make_input(
                    "acc-snap",
                    TransactionKind::Income,
                    day * 100,
                    &format!("2026-01-0{day}"),
                ),
            )
            .unwrap()
            .id,
        );
    }

    let victim = ids[2].clone();
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "SELECT id,kind,amount_cents",
        &[&format!(
            "UPDATE transactions SET is_deleted=1 WHERE id = '{victim}'"
        )],
    );

    let result = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中 items 查询（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert!(
        !result.items.is_empty(),
        "种子应产出流水（空列表会让口径断言空转）"
    );
    assert_eq!(
        result.total as usize,
        result.items.len(),
        "total 与 items 必须同快照：COUNT 之后、items 之前落了写提交时页与总数错位"
    );
}

#[test]
fn search_items_and_total_share_one_snapshot() {
    let dir = ScratchDir::new("tx-search-read-snapshot");
    let conn = test_support::open_file(dir.path());
    test_support::seed_account(&conn, "acc-snap", "现金", "cash", "CNY", 0);
    let mut ids: Vec<String> = Vec::new();
    for day in 1..=3i64 {
        ids.push(
            create_transaction_internal(
                &conn,
                make_input(
                    "acc-snap",
                    TransactionKind::Income,
                    day * 100,
                    &format!("2026-01-0{day}"),
                ),
            )
            .unwrap()
            .id,
        );
    }

    let baseline =
        search_transactions_internal(&conn, "", 1, 10, Some(0), None, None, None).unwrap();

    let victim = ids[1].clone();
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "WHERE t.id IN",
        &[&format!(
            "UPDATE transactions SET amount_cents = amount_cents + 7 WHERE id = '{victim}'"
        )],
    );
    let after = search_transactions_internal(&conn, "", 1, 10, Some(0), None, None, None).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中回表查询（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        baseline.total, after.total,
        "命中总数与回表页必须同快照（总数口径不变的注入下漂移即窗口）"
    );
    let amounts = |r: &TransactionSearchResult| -> Vec<(String, i64)> {
        r.items
            .iter()
            .map(|t| (t.id.clone(), t.amount_cents))
            .collect()
    };
    assert!(
        !baseline.items.is_empty(),
        "种子应产出命中行（空结果会让口径断言空转）"
    );
    assert_eq!(
        amounts(&baseline),
        amounts(&after),
        "回表页金额必须与基线同时点——注入写要么整体进快照、要么整体不进"
    );
}
