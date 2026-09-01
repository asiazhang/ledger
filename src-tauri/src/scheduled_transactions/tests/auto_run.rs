//! 自动执行（追补）入口（issue #307 / ADR-0042）：唯一新增接缝的行为钉子。
//!
//! 全部测试打同一个追补入口——内存库 + 注入开关状态与今天日期，只断言外部可观察
//! 行为：执行汇总与数据库终态（期次状态、交易行、脏标记）。不测线程/sleep/锁
//! （周期调用结构由调度线程「只做周期调用」保证，与自动备份同款纪律）。

use super::super::*;
use super::common::{create_subscription, insert_account, occurrence_status, setup_db};
use chrono::NaiveDate;
use rusqlite::{Connection, params};

/// 解析注入的「今天」。
fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("合法日期")
}

/// 以注入的开关与今天执行追补入口。
fn catch_up(conn: &Connection, enabled: bool, today: &str) -> CatchUpSummary {
    run_catch_up(conn, enabled, day(today))
}

/// 计划全部期次的（计划日期, 状态, 回填交易）清单，按计划日期升序。
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

/// 计划生成的交易日期清单（经期次回填关联），按交易日期升序。
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

/// 整库现存交易笔数。
fn txn_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

/// 直接把某条期次置为指定状态（构造 failed / processing 等非 pending 前置）。
fn set_status(conn: &Connection, occ_id: &str, status: &str) {
    conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status=?2 WHERE id=?1",
        params![occ_id, status],
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// 开关关闭：空转
// ---------------------------------------------------------------------------

/// 开关关闭时追补空转：不动任何期次、不产生交易（验收①）。
#[test]
fn disabled_means_idle() {
    let conn = setup_db();
    insert_account(&conn, "acc-cny", "CNY");
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

// ---------------------------------------------------------------------------
// 到期追补：交易日期 = 计划日期，含今天边界，跨午夜滚动
// ---------------------------------------------------------------------------

/// 开启后到期期次逐条落账，交易日期忠实取期次计划日期（验收②）；
/// 再次注入更晚的「今天」模拟跨午夜后的下一轮，滚动追补新到期期次。
#[test]
fn due_executed_with_plan_date_and_rolls_on_later_days() {
    let conn = setup_db();
    insert_account(&conn, "acc-cny", "CNY");
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

    // 跨午夜后的下一轮：只滚动到新到期的 03-15 一期，已完成的不再重跑。
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

/// 「今天」含边界：计划日期恰为今天的期次当轮即被追补。
#[test]
fn today_boundary_is_due() {
    let conn = setup_db();
    insert_account(&conn, "acc-cny", "CNY");
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

/// 未到期（计划日期 > 今天）不动：明天才到期的期次留在 pending。
#[test]
fn future_occurrences_untouched() {
    let conn = setup_db();
    insert_account(&conn, "acc-cny", "CNY");
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

// ---------------------------------------------------------------------------
// 状态矩阵：非 pending 期次与非 active 计划不碰
// ---------------------------------------------------------------------------

/// failed / processing / cancelled 期次一律不被追补，其余 pending 正常补齐（验收③）。
#[test]
fn non_pending_occurrences_untouched() {
    let conn = setup_db();
    insert_account(&conn, "acc-cny", "CNY");
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

/// 暂停与取消的计划不被追补（验收③）：期次与交易保持原状。
#[test]
fn paused_and_cancelled_plans_untouched() {
    let conn = setup_db();
    insert_account(&conn, "acc-paused", "CNY");
    insert_account(&conn, "acc-cancelled", "CNY");
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

// ---------------------------------------------------------------------------
// 失败语义：单期失败不中断后续，失败期次置 failed 不被自动反复重试
// ---------------------------------------------------------------------------

/// 无汇率的外币计划逐期失败并被置为 failed，同批其他计划不受影响；
/// 后续轮次不重试已 failed 的期次（保持手动重试，验收③）。
#[test]
fn failure_marks_failed_continues_and_never_auto_retries() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD"); // 无汇率：归一化必失败
    insert_account(&conn, "acc-cny", "CNY");
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

    // 下一轮：已 failed 的期次不被自动反复重试；新到期期次照常处理。
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

// ---------------------------------------------------------------------------
// 备份联动：成功落账置脏
// ---------------------------------------------------------------------------

/// 追补成功落账经统一写入口语义置脏，自动备份到期判定联动可见（验收④）；
/// 空转与未到期轮次不动脏标记。
#[test]
fn success_marks_dirty_for_backup_linkage() {
    let conn = setup_db();
    insert_account(&conn, "acc-cny", "CNY");
    let plan_id = create_subscription(&conn, "acc-cny", "CNY", 3000, None);

    catch_up(&conn, false, "2026-02-20");
    assert!(
        !crate::auto_backup::get_state(&conn).unwrap().dirty,
        "空转不置脏"
    );

    catch_up(&conn, true, "2026-01-14");
    assert!(
        !crate::auto_backup::get_state(&conn).unwrap().dirty,
        "未到期不落账不置脏"
    );

    catch_up(&conn, true, "2026-02-20");
    assert!(
        crate::auto_backup::get_state(&conn).unwrap().dirty,
        "成功落账应置脏联动自动备份判定"
    );
    let (status, tx) = occurrence_status(&conn, &occurrence_id_at(&conn, &plan_id, 0));
    assert_eq!(status, "completed");
    assert!(tx.is_some());
}

/// 取计划第 n 条（按计划日期升序）期次 id。
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
