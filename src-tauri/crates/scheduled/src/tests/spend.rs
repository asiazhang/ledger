use super::common::{
    create_installment, create_subscription, create_subscription_cycle, create_transfer_plan,
    read_txn,
};
use rusqlite::Connection;
use rusqlite::params;
use tauri_app_lib::ledger_scheduled::*;
use tauri_app_lib::test_support;

use tauri_app_lib::ledger_scheduled::query_subscription_spend;

fn execute_first_n_occurrences(conn: &Connection, plan_id: &str, n: usize) -> Vec<String> {
    let occ_ids: Vec<String> = get_plan_detail(conn, plan_id)
        .unwrap()
        .pending_occurrences
        .into_iter()
        .take(n)
        .map(|o| o.id)
        .collect();
    occ_ids
        .iter()
        .map(|id| read_txn(conn, &execute_occurrence(conn, id).unwrap()).date)
        .collect()
}

fn date(s: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn month_cents(overview: &SubscriptionSpendOverview, month: &str) -> i64 {
    overview
        .months
        .iter()
        .find(|m| m.month == month)
        .unwrap_or_else(|| panic!("12 个月序列应包含 {month}"))
        .native_cents
}

#[test]
fn subscription_spend_aggregates_by_calendar_month() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));
    execute_first_n_occurrences(&conn, &plan_id, 2);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(month_cents(&overview, "2026-01"), 3000);
    assert_eq!(month_cents(&overview, "2026-02"), 3000);
    assert_eq!(month_cents(&overview, "2026-03"), 0, "未扣款月应为 0");
    assert_eq!(overview.this_month_native_cents, 0, "本月（2026-03）无扣款");
    assert_eq!(overview.this_year_native_cents, 6000);
    assert_eq!(overview.months.len(), 12, "固定 12 个月槽位");
    assert_eq!(overview.months[0].month, "2025-04", "旧→新，含当月");
    assert_eq!(overview.months[11].month, "2026-03");
}

#[test]
fn subscription_spend_yearly_not_amortized() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: "acc".into(),
            category_id: None,
            amount_cents: 34800,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Yearly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-01-10".into(),
            note: Some("云存储年费".into()),
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap();
    execute_first_n_occurrences(&conn, &plan_id, 1);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(month_cents(&overview, "2026-01"), 34800, "扣款月全额计入");
    assert_eq!(month_cents(&overview, "2026-02"), 0, "不摊销");
    assert_eq!(month_cents(&overview, "2026-03"), 0, "不摊销");
    assert_eq!(overview.this_month_native_cents, 0);
    assert_eq!(overview.this_year_native_cents, 34800);
}

#[test]
fn subscription_spend_keeps_cancelled_history_and_excludes_other_kinds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let sub_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));
    execute_first_n_occurrences(&conn, &sub_id, 2);
    update_plan_status(&conn, &sub_id, ScheduledStatus::Cancelled).unwrap();

    let inst_id = create_installment(&conn, "acc", 3100, 3);
    execute_first_n_occurrences(&conn, &inst_id, 1);
    let transfer_id = {
        test_support::seed_account(&conn, "acc2", "acc2", "cash", "CNY", 0);
        create_transfer_plan(&conn, "acc", "acc2", 50000)
    };
    execute_first_n_occurrences(&conn, &transfer_id, 1);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(month_cents(&overview, "2026-01"), 3000, "取消后历史保留");
    assert_eq!(month_cents(&overview, "2026-02"), 3000, "取消后历史保留");
    assert_eq!(overview.this_year_native_cents, 6000, "分期/转账不计入");

    assert_eq!(overview.rows.len(), 1, "只统计订阅计划");
    let row = &overview.rows[0];
    assert_eq!(row.plan_id, sub_id);
    assert_eq!(row.status, "cancelled");
    assert_eq!(row.this_month_native_cents, 0);
    assert_eq!(row.this_year_native_cents, 6000);
}

#[test]
fn subscription_projected_spend_coefficients() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        3000,
        RecurrenceType::Monthly,
        1,
        Some("月付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        34800,
        RecurrenceType::Yearly,
        1,
        Some("年付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        5200,
        RecurrenceType::Weekly,
        1,
        Some("周付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        300,
        RecurrenceType::Daily,
        1,
        Some("日付"),
    );
    create_subscription_cycle(
        &conn,
        "acc",
        "CNY",
        3000,
        RecurrenceType::Monthly,
        3,
        Some("每三月"),
    );

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(overview.projected_month_native_cents, 38433);
    assert_eq!(
        overview.projected_year_native_cents,
        38433 * 12,
        "折算年成本 = 折算月成本 × 12"
    );
}

#[test]
fn subscription_projected_spend_counts_only_active_plans() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    create_subscription(&conn, "acc", "CNY", 3000, Some("进行中"));
    let paused = create_subscription(&conn, "acc", "CNY", 5000, Some("已暂停"));
    update_plan_status(&conn, &paused, ScheduledStatus::Paused).unwrap();
    let cancelled = create_subscription(&conn, "acc", "CNY", 7000, Some("已取消"));
    update_plan_status(&conn, &cancelled, ScheduledStatus::Cancelled).unwrap();

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(
        overview.projected_month_native_cents, 3000,
        "暂停/取消不计入，未执行也计入"
    );
    assert_eq!(overview.projected_year_native_cents, 36000);
    assert_eq!(overview.this_month_native_cents, 0);
    assert_eq!(overview.this_year_native_cents, 0);
}

#[test]
fn subscription_projected_spend_converts_and_requires_rate() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    create_subscription(&conn, "acc-usd", "USD", 10000, Some("国际订阅"));

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(
        overview.projected_month_native_cents, 72000,
        "10000 × 7.2 折算本位币"
    );

    conn.execute("DELETE FROM exchange_rates", params![])
        .unwrap();
    let err = query_subscription_spend(&conn, date("2026-03-20")).unwrap_err();
    assert!(err.to_string().contains("汇率"), "缺汇率应报错上抛: {err}");
}

#[test]
fn subscription_spend_uses_native_amounts_from_transactions() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    test_support::seed_fx_rate_history(&conn, "fxh-spend", "USD", "CNY", "2026-01-12", 7.2);
    let plan_id = create_subscription(&conn, "acc-usd", "USD", 10000, Some("国际订阅"));
    execute_first_n_occurrences(&conn, &plan_id, 1);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(
        month_cents(&overview, "2026-01"),
        72000,
        "应取流水 amount_native_cents（10000 × 7.2）"
    );
    let row = &overview.rows[0];
    assert_eq!(row.amount_cents, 10000, "行内原始金额保持计划币种");
    assert_eq!(row.currency_code, "USD");
    assert_eq!(row.this_year_native_cents, 72000);
}

#[test]
fn subscription_spend_aggregates_expense_net_from_matrix() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));
    execute_first_n_occurrences(&conn, &plan_id, 1);
    let expense_txn_id: String = conn
        .query_row(
            "SELECT transaction_id FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id = ?1 AND status = 'completed'",
            params![plan_id],
            |r| r.get(0),
        )
        .unwrap();

    insert_occurrence_txn(&conn, &plan_id, "2026-01-20", "income", 8888, None);
    insert_occurrence_txn(
        &conn,
        &plan_id,
        "2026-01-25",
        "refund",
        1200,
        Some(&expense_txn_id),
    );

    let overview = query_subscription_spend(&conn, date("2026-02-20")).unwrap();
    assert_eq!(
        month_cents(&overview, "2026-01"),
        3000 - 1200,
        "income 不混入合计，refund 按支出净额冲减"
    );
}

fn insert_occurrence_txn(
    conn: &Connection,
    plan_id: &str,
    sched_date: &str,
    kind: &str,
    amount_cents: i64,
    refund_of: Option<&str>,
) {
    let txn_id = format!("drift-{sched_date}-{kind}");
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,'acc',NULL,NULL,?4,NULL,?5,?6,?6,1,'test',0)",
        params![txn_id, kind, amount_cents, refund_of, sched_date, test_support::FIXED_NOW],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO scheduled_transaction_occurrences \
         (id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'completed',?4,?5,?6,?6,1,'test',0)",
        params![
            format!("occ-{sched_date}-{kind}"),
            plan_id,
            sched_date,
            txn_id,
            amount_cents,
            test_support::FIXED_NOW
        ],
    )
    .unwrap();
}
