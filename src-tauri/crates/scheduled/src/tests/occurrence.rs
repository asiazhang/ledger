use super::super::*;
use super::common::{
    create_installment, create_subscription, create_transfer_plan, first_pending_occurrence,
    occurrence_status, read_txn,
};
use rusqlite::{Connection, params};
use tauri_app_lib::test_support;

#[test]
fn execute_occurrence_converts_non_default_currency_to_native() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-occ", "USD", "CNY", "2026-01-12", 7.2);
    let plan_id = create_subscription(&conn, "acc-usd", "USD", 10000, Some("国际订阅"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();

    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "expense");
    assert_eq!(txn.amount_cents, 10000, "原始币种金额保留");
    assert_eq!(txn.currency_code, "USD");
    assert_eq!(
        txn.amount_native_cents, 72000,
        "本位币金额应经 convert_to_native_on_trade_date 按交易日折算"
    );
    assert_eq!(txn.account_id, "acc-usd");
    assert_eq!(txn.to_account_id, None);
    assert_eq!(txn.note.as_deref(), Some("国际订阅"));
    assert_eq!(txn.date, "2026-01-15");
}

#[test]
fn execute_occurrence_errors_without_rate_for_non_default_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-jpy", "acc-jpy", "cash", "JPY", 0);
    let plan_id = create_subscription(&conn, "acc-jpy", "JPY", 10000, None);
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let err = execute_occurrence(&conn, &occ_id).unwrap_err();
    assert!(err.to_string().contains("汇率"), "实际: {err}");

    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "pending", "期次不应滞留 processing");
    assert_eq!(backfilled, None, "失败不应回填交易 id");
    test_support::seed_fx_rate_history(&conn, "fxh-occ-retry", "JPY", "CNY", "2026-01-12", 0.05);
    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.amount_native_cents, 500);
}

#[test]
fn execute_occurrence_default_currency_stays_1_1() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc", "CNY", 6600, Some("视频会员"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.amount_cents, 6600);
    assert_eq!(txn.amount_native_cents, 6600, "默认币种应 1:1");
}

fn inject_txn_insert_failure(conn: &Connection) {
    conn.execute(
        "CREATE TRIGGER block_txn_insert BEFORE INSERT ON transactions \
         BEGIN SELECT RAISE(ABORT, '测试注入：期次落库失败'); END",
        [],
    )
    .unwrap();
}

#[test]
fn execute_occurrence_mid_failure_rolls_back_to_pending() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("回滚订阅"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);
    inject_txn_insert_failure(&conn);

    let err = execute_occurrence(&conn, &occ_id).unwrap_err();
    assert!(
        err.to_string().contains("测试注入"),
        "应上抛注入的落库错误，实际: {err:?}"
    );

    let txn_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(txn_count, 0, "中途失败不应残留交易行");
    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "pending", "期次应回原状态可重试，不滞留 processing");
    assert_eq!(backfilled, None, "失败不应回填交易 id");

    conn.execute("DROP TRIGGER block_txn_insert", []).unwrap();
    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "completed");
    assert_eq!(backfilled.as_deref(), Some(txn_id.as_str()));
}

#[test]
fn execute_subscription_maps_expense_and_completes_occurrence() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("云服务"));
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();

    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "expense");
    assert_eq!(txn.account_id, "acc");
    assert_eq!(txn.category_id, None);
    assert_eq!(txn.note.as_deref(), Some("云服务"));
    assert_eq!(txn.date, "2026-01-15");
    assert_eq!(txn.refund_of_transaction_id, None);

    let (status, backfilled) = occurrence_status(&conn, &occ_id);
    assert_eq!(status, "completed", "期次应流转为 completed");
    assert_eq!(
        backfilled.as_deref(),
        Some(txn_id.as_str()),
        "期次应回填交易 id"
    );
}

#[test]
fn execute_subscription_inherits_category() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: "acc".into(),
            category_id: Some("95d6dc66-12c4-5f2b-bf9b-1d439a9c8100".into()),
            amount_cents: 3000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-15".into(),
            note: None,
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap();
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();
    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "expense");
    assert_eq!(
        txn.category_id.as_deref(),
        Some("95d6dc66-12c4-5f2b-bf9b-1d439a9c8100"),
        "支出交易应继承计划分类"
    );
}

#[test]
fn execute_transfer_maps_out_and_in_accounts() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let plan_id = create_transfer_plan(&conn, "acc-a", "acc-b", 50000);
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let txn_id = execute_occurrence(&conn, &occ_id).unwrap();

    let txn = read_txn(&conn, &txn_id);
    assert_eq!(txn.kind, "transfer");
    assert_eq!(txn.account_id, "acc-a", "转出账户");
    assert_eq!(txn.to_account_id.as_deref(), Some("acc-b"), "转入账户");
    assert_eq!(txn.category_id, None);
    assert_eq!(txn.amount_native_cents, 50000);
}

#[test]
fn execute_installments_all_marks_plan_completed() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_installment(&conn, "acc", 3100, 3);
    let occ_ids: Vec<String> = get_plan_detail(&conn, &plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .map(|o| o.id)
        .collect();
    assert_eq!(occ_ids.len(), 3);

    let mut txns = Vec::new();
    for occ_id in &occ_ids {
        txns.push(execute_occurrence(&conn, occ_id).unwrap());
    }

    let amounts: Vec<i64> = txns
        .iter()
        .map(|id| read_txn(&conn, id).amount_cents)
        .collect();
    assert_eq!(amounts, vec![1033, 1033, 1034]);
    let mut distinct = txns.clone();
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "每期应落独立交易");

    let plan_status: String = conn
        .query_row(
            "SELECT status FROM scheduled_transactions WHERE id=?1",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plan_status, "completed", "全部期次完成应置计划为 completed");
    for occ_id in &occ_ids {
        assert_eq!(occurrence_status(&conn, occ_id).0, "completed");
    }
}

#[test]
fn execute_occurrence_rejects_paused_plan() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, None);
    update_plan_status(&conn, &plan_id, ScheduledStatus::Paused).unwrap();
    let occ_id = first_pending_occurrence(&conn, &plan_id);

    let err = execute_occurrence(&conn, &occ_id).unwrap_err();
    assert_eq!(err.to_string(), "关联计划未处于活跃状态");
}

#[test]
fn execute_occurrence_with_preexisting_landing_completes_without_second_row() {
    use ledger_transaction::write::writer;

    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "现金", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc-a", "CNY", 3000, None);
    let occ_id = first_pending_occurrence(&conn, &plan_id);
    let date: String = conn
        .query_row(
            "SELECT scheduled_date FROM scheduled_transaction_occurrences WHERE id=?1",
            [&occ_id],
            |r| r.get(0),
        )
        .unwrap();
    let landing = occurrence_transaction_id(&plan_id, &date);

    let norm = writer::normalize(
        &conn,
        &writer::Input {
            funding: Vec::new(),
            existing_funding: Vec::new(),
            kind: ledger_transaction::amount::TransactionKind::Expense,
            amount_cents: 3000,
            currency_code: "CNY".into(),
            account_id: Some("acc-a".into()),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            existing_merchant_id: None,
            policy_id: None,
            existing_policy_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            source_order_no: None,
            note: None,
            date: date.clone(),
            fx_rate: None,
            fx_edit_baseline: None,
            purchases: Vec::new(),
            existing_purchases: Vec::new(),
        },
    )
    .unwrap();
    writer::insert_row_with_id(&conn, &landing, &norm).unwrap();

    let again = execute_occurrence(&conn, &occ_id).unwrap();
    assert_eq!(again, landing, "执行复用确定性落地身份");
    assert_eq!(
        occurrence_status(&conn, &occ_id),
        ("completed".into(), Some(landing)),
        "期次完成并回填既有落地"
    );
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "同一期次只落一笔");
    assert!(
        !ledger_sync_engine::read_ops(&conn)
            .unwrap()
            .iter()
            .any(|op| matches!(
                &op.command,
                ledger_sync_engine::DomainCommand::Scheduled(
                    tauri_app_lib::ledger_scheduled::ScheduledCommand::ExecuteOccurrence { .. }
                )
            )),
        "已落地路径不产出期次 op"
    );
}
