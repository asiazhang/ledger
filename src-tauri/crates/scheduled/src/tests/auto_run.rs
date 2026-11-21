use super::common::{create_subscription, occurrence_status};
use chrono::NaiveDate;
use rusqlite::{Connection, params};
use tauri_app_lib::ledger_scheduled::*;
use tauri_app_lib::test_support;

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("合法日期")
}

fn catch_up(conn: &Connection, enabled: bool, today: &str) -> CatchUpSummary {
    run_catch_up(conn, enabled, day(today))
}

fn occurrence_rows(conn: &Connection, plan_id: &str) -> Vec<(String, String, Option<String>)> {
    let mut stmt = conn
        .prepare(
            "SELECT scheduled_date,status,transaction_id FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id=?1 AND is_deleted=0 ORDER BY scheduled_date ASC",
        )
        .unwrap();
    stmt.query_map(params![plan_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

fn plan_txn_dates(conn: &Connection, plan_id: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT t.date FROM transactions t \
             JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
             WHERE o.scheduled_transaction_id=?1 AND t.is_deleted=0 ORDER BY t.date ASC",
        )
        .unwrap();
    stmt.query_map(params![plan_id], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

fn txn_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn set_status(conn: &Connection, occ_id: &str, status: &str) {
    conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status=?2 WHERE id=?1",
        params![occ_id, status],
    )
    .unwrap();
}

#[test]
fn disabled_means_idle() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc-cny", "CNY", 3000, None);

    let summary = catch_up(&conn, false, "2026-03-20");

    assert_eq!(summary, CatchUpSummary::default(), "空转汇总应全零");
    let rows = occurrence_rows(&conn, &plan_id);
    assert_eq!(rows.len(), 12, "期次不应被动过");
    assert!(
        rows.iter()
            .all(|(_, st, tx)| st == "pending" && tx.is_none())
    );
    assert_eq!(txn_count(&conn), 0, "不应产生交易");
}

#[test]
fn due_executed_with_plan_date_and_rolls_on_later_days() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc-cny", "CNY", 3000, None);

    let summary = catch_up(&conn, true, "2026-02-20");
    assert_eq!(
        summary,
        CatchUpSummary {
            due: 2,
            executed: 2,
            failed: 0,
            failures: vec![],
        },
        "01-15 与 02-15 两期应到期"
    );
    assert_eq!(
        plan_txn_dates(&conn, &plan_id),
        vec!["2026-01-15".to_string(), "2026-02-15".to_string()],
        "交易日期应回填期次计划日期"
    );
    let rows = occurrence_rows(&conn, &plan_id);
    assert_eq!(rows[0].1, "completed");
    assert_eq!(rows[1].1, "completed");
    assert!(rows[2..].iter().all(|(_, st, _)| st == "pending"));

    let next = catch_up(&conn, true, "2026-03-20");
    assert_eq!(next.due, 1);
    assert_eq!(next.executed, 1);
    assert_eq!(
        plan_txn_dates(&conn, &plan_id),
        vec![
            "2026-01-15".to_string(),
            "2026-02-15".to_string(),
            "2026-03-15".to_string(),
        ]
    );
}

#[test]
fn today_boundary_is_due() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let plan_id = create_plan(
        &conn,
        CreateScheduledInput {
            kind: ScheduledKind::Subscription,
            account_id: "acc-cny".into(),
            category_id: None,
            amount_cents: 3000,
            currency_code: "CNY".into(),
            recurrence_type: RecurrenceType::Monthly,
            recurrence_interval: 1,
            recurrence_day: None,
            start_date: "2026-03-20".into(),
            note: None,
            merchant_id: None,
            policy_id: None,
            total_amount_cents: None,
            total_occurrences: None,
            to_account_id: None,
        },
    )
    .unwrap();

    let summary = catch_up(&conn, true, "2026-03-20");
    assert_eq!(summary.due, 1, "计划日期 ≤ 今天（含今天）应到期");
    assert_eq!(summary.executed, 1);
    assert_eq!(
        plan_txn_dates(&conn, &plan_id),
        vec!["2026-03-20".to_string()]
    );
}

#[test]
fn future_occurrences_untouched() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc-cny", "CNY", 3000, None);

    let summary = catch_up(&conn, true, "2026-01-14");
    assert_eq!(summary.due, 0);
    assert_eq!(summary.executed, 0);
    assert_eq!(txn_count(&conn), 0);
    assert!(
        occurrence_rows(&conn, &plan_id)
            .iter()
            .all(|(_, st, _)| st == "pending")
    );
}

#[test]
fn non_pending_occurrences_untouched() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc-cny", "CNY", 3000, None);
    let (failed_id, processing_id, cancelled_id) = (
        occurrence_id_at(&conn, &plan_id, 0),
        occurrence_id_at(&conn, &plan_id, 1),
        occurrence_id_at(&conn, &plan_id, 2),
    );
    set_status(&conn, &failed_id, "failed");
    set_status(&conn, &processing_id, "processing");
    set_status(&conn, &cancelled_id, "cancelled");

    let summary = catch_up(&conn, true, "2026-12-31");

    assert_eq!(summary.due, 9, "只统计 pending 到期期次");
    assert_eq!(summary.executed, 9);
    assert_eq!(summary.failed, 0);
    let rows = occurrence_rows(&conn, &plan_id);
    assert_eq!(rows[0].1, "failed", "failed 期次不被碰");
    assert_eq!(rows[1].1, "processing", "processing 期次不被碰");
    assert_eq!(rows[2].1, "cancelled", "cancelled 期次不被碰");
    assert!(rows[0].2.is_none() && rows[1].2.is_none() && rows[2].2.is_none());
    assert!(
        rows[3..]
            .iter()
            .all(|(_, st, tx)| st == "completed" && tx.is_some())
    );
}

#[test]
fn paused_and_cancelled_plans_untouched() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-paused", "acc-paused", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-cancelled", "acc-cancelled", "cash", "CNY", 0);
    let paused = create_subscription(&conn, "acc-paused", "CNY", 3000, None);
    let cancelled = create_subscription(&conn, "acc-cancelled", "CNY", 3000, None);
    update_plan_status(&conn, &paused, ScheduledStatus::Paused).unwrap();
    update_plan_status(&conn, &cancelled, ScheduledStatus::Cancelled).unwrap();

    let summary = catch_up(&conn, true, "2026-12-31");

    assert_eq!(summary, CatchUpSummary::default(), "非 active 计划不到期");
    assert!(
        occurrence_rows(&conn, &paused)
            .iter()
            .all(|(_, st, _)| st == "pending"),
        "暂停计划的期次保持 pending"
    );
    assert!(
        occurrence_rows(&conn, &cancelled)
            .iter()
            .all(|(_, st, _)| st == "cancelled"),
        "取消计划的期次保持 cancelled"
    );
    assert_eq!(txn_count(&conn), 0);
}

#[test]
fn failure_marks_failed_continues_and_never_auto_retries() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let usd = create_subscription(&conn, "acc-usd", "USD", 10000, Some("缺汇率订阅"));
    let cny = create_subscription(&conn, "acc-cny", "CNY", 2000, Some("正常订阅"));

    let summary = catch_up(&conn, true, "2026-02-20");
    assert_eq!(summary.due, 4);
    assert_eq!(summary.executed, 2, "单期失败不中断同批后续");
    assert_eq!(summary.failed, 2);
    assert_eq!(summary.failures.len(), 2, "失败明细应随汇总返回");

    let usd_rows = occurrence_rows(&conn, &usd);
    assert!(
        usd_rows[..2]
            .iter()
            .all(|(_, st, tx)| st == "failed" && tx.is_none()),
        "追补尝试失败的期次应置为 failed 待手动重试"
    );
    let cny_rows = occurrence_rows(&conn, &cny);
    assert!(cny_rows[..2].iter().all(|(_, st, _)| st == "completed"));

    let next = catch_up(&conn, true, "2026-03-20");
    assert_eq!(next.due, 2, "只滚动到 3 月新到期的一期");
    assert_eq!(next.executed, 1);
    assert_eq!(next.failed, 1);
    let usd_rows = occurrence_rows(&conn, &usd);
    assert_eq!(usd_rows[0].1, "failed", "已 failed 期次不被重试");
    assert_eq!(usd_rows[1].1, "failed", "已 failed 期次不被重试");
    assert_eq!(
        plan_txn_dates(&conn, &usd),
        Vec::<String>::new(),
        "失败计划不产生交易"
    );
}

#[test]
fn success_marks_dirty_for_backup_linkage() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let plan_id = create_subscription(&conn, "acc-cny", "CNY", 3000, None);

    catch_up(&conn, false, "2026-02-20");
    assert!(
        !ledger_backup::get_state(&conn).unwrap().dirty,
        "空转不置脏"
    );

    catch_up(&conn, true, "2026-01-14");
    assert!(
        !ledger_backup::get_state(&conn).unwrap().dirty,
        "未到期不落账不置脏"
    );

    catch_up(&conn, true, "2026-02-20");
    assert!(
        ledger_backup::get_state(&conn).unwrap().dirty,
        "成功落账应置脏联动自动备份判定"
    );
    let (status, tx) = occurrence_status(&conn, &occurrence_id_at(&conn, &plan_id, 0));
    assert_eq!(status, "completed");
    assert!(tx.is_some());
}

fn occurrence_id_at(conn: &Connection, plan_id: &str, index: usize) -> String {
    let mut stmt = conn
        .prepare(
            "SELECT id FROM scheduled_transaction_occurrences \
             WHERE scheduled_transaction_id=?1 AND is_deleted=0 ORDER BY scheduled_date ASC LIMIT 1 OFFSET ?2",
        )
        .unwrap();
    stmt.query_row(params![plan_id, index as i64], |r| r.get(0))
        .unwrap()
}
