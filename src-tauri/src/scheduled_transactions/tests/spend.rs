//! 订阅花费双口径（ADR-0023，issue #160 / #161）：实际花费按期次流水逐月忠实统计
//! （决策二），推算成本按周期系数折算（issue #161），两口径并行互不影响。

use super::super::*;
use super::common::{
    create_installment, create_subscription, create_subscription_cycle, create_transfer_plan,
    insert_account, insert_rate, read_txn, setup_db,
};
use rusqlite::Connection;
use rusqlite::params;

// ---------------------------------------------------------------------------
// 订阅花费——实际花费口径（issue #160，ADR-0023 决策二）
// ---------------------------------------------------------------------------

use crate::scheduled_transactions::query_subscription_spend;

/// 执行计划前 N 条 pending 期次（scheduled_date 升序），返回生成的交易日期。
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

/// 实际花费按期次流水逐月忠实统计（本位币），非扣款月补 0；不摊销。
#[test]
fn subscription_spend_aggregates_by_calendar_month() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let plan_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));
    // 2026-01-15 起月付，执行前两期 → 2026-01 / 2026-02 各一笔 3000
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

/// 年付订阅不摊销：扣款月全额计入，其余月份为 0。
#[test]
fn subscription_spend_yearly_not_amortized() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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

/// 计划取消/暂停不影响历史实际花费；非订阅计划（分期/定时转账）不计入。
#[test]
fn subscription_spend_keeps_cancelled_history_and_excludes_other_kinds() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let sub_id = create_subscription(&conn, "acc", "CNY", 3000, Some("视频会员"));
    execute_first_n_occurrences(&conn, &sub_id, 2);
    update_plan_status(&conn, &sub_id, ScheduledStatus::Cancelled).unwrap();

    // 干扰项：分期与定时转账各执行一期，不应计入订阅花费
    let inst_id = create_installment(&conn, "acc", 3100, 3);
    execute_first_n_occurrences(&conn, &inst_id, 1);
    let transfer_id = {
        insert_account(&conn, "acc2", "CNY");
        create_transfer_plan(&conn, "acc", "acc2", 50000)
    };
    execute_first_n_occurrences(&conn, &transfer_id, 1);

    let overview = query_subscription_spend(&conn, date("2026-03-20")).unwrap();
    assert_eq!(month_cents(&overview, "2026-01"), 3000, "取消后历史保留");
    assert_eq!(month_cents(&overview, "2026-02"), 3000, "取消后历史保留");
    assert_eq!(overview.this_year_native_cents, 6000, "分期/转账不计入");

    // 逐订阅行：取消计划仍在行内，行内本月/本年口径正确
    assert_eq!(overview.rows.len(), 1, "只统计订阅计划");
    let row = &overview.rows[0];
    assert_eq!(row.plan_id, sub_id);
    assert_eq!(row.status, "cancelled");
    assert_eq!(row.this_month_native_cents, 0);
    assert_eq!(row.this_year_native_cents, 6000);
}

/// 推算成本（issue #161）：各周期系数折算正确（月 ×1、年 ÷12、周 ×52÷12、日 ×30），
/// recurrence_interval > 1 时按间隔均摊；折算年成本 = 折算月成本 × 12。
#[test]
fn subscription_projected_spend_coefficients() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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
    // 3000×1 + 34800÷12 + 5200×52÷12 + 300×30 + 3000÷3 = 3000 + 2900 + 22533 + 9000 + 1000
    assert_eq!(overview.projected_month_native_cents, 38433);
    assert_eq!(
        overview.projected_year_native_cents,
        38433 * 12,
        "折算年成本 = 折算月成本 × 12"
    );
}

/// 推算成本只统计 active 计划（暂停/取消不计入），且不看执行情况（未执行也计入）。
#[test]
fn subscription_projected_spend_counts_only_active_plans() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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
    // 推算口径不影响实际口径：均未执行，实际花费为 0
    assert_eq!(overview.this_month_native_cents, 0);
    assert_eq!(overview.this_year_native_cents, 0);
}

/// 推算成本在计划币种上折算本位币；缺汇率时报错上抛，不静默混算。
#[test]
fn subscription_projected_spend_converts_and_requires_rate() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
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

/// 非默认币种订阅按流水的本位币金额（落库时折算）计入，不二次折算。
#[test]
fn subscription_spend_uses_native_amounts_from_transactions() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
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
