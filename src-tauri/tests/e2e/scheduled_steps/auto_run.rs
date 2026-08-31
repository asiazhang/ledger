//! 自动执行（追补，issue #307 / ADR-0042）的 BDD 步骤：步骤直调追补入口并注入
//! 开关与今天日期（与备份 feature 步骤直调备份入口的 seam 一致，S3 定案）。
//! 断言只看外部可观察行为：执行汇总、期次状态、生成交易的日期。

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::scheduled_transactions::run_catch_up;

use crate::world::LedgerWorld;

/// 以注入的今日执行追补（开关开启）。
#[when(expr = "以 {string} 为今日执行自动追补")]
fn run_catchup_enabled(world: &mut LedgerWorld, today: String) {
    run_catchup(world, true, &today);
}

/// 以注入的今日执行追补（开关关闭——空转语义场景）。
#[when(expr = "自动执行关闭时以 {string} 为今日执行追补")]
fn run_catchup_disabled(world: &mut LedgerWorld, today: String) {
    run_catchup(world, false, &today);
}

fn run_catchup(world: &mut LedgerWorld, enabled: bool, today: &str) {
    let day = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d").expect("日期应为 YYYY-MM-DD");
    world.last_catch_up = Some(run_catch_up(&world_conn!(world), enabled, day));
}

#[then(expr = "追补汇总应为 到期 {int} 成功 {int} 失败 {int}")]
fn assert_catchup_summary(world: &mut LedgerWorld, due: usize, executed: usize, failed: usize) {
    let summary = world.last_catch_up.as_ref().expect("尚未执行追补");
    assert_eq!(
        (summary.due, summary.executed, summary.failed),
        (due, executed, failed),
        "追补汇总不符：{summary:?}"
    );
}

/// 最近计划生成的交易日期应依次为给定清单（按交易日期升序）。
#[then(expr = "最近计划生成的交易日期应依次为 {string}")]
fn assert_plan_txn_dates(world: &mut LedgerWorld, dates_csv: String) {
    let plan_id = world.last_plan_id.clone().expect("尚无定时计划");
    let expected: Vec<String> = dates_csv
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let dates: Vec<String> = {
        let conn = world_conn!(world);
        let mut stmt = conn
            .prepare(
                "SELECT t.date FROM transactions t \
                 JOIN scheduled_transaction_occurrences o ON o.transaction_id=t.id \
                 WHERE o.scheduled_transaction_id=?1 AND t.is_deleted=0 \
                 ORDER BY t.date ASC, t.created_at ASC",
            )
            .unwrap();
        stmt.query_map(params![plan_id], |r| r.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    assert_eq!(dates, expected, "计划生成交易的日期不符");
}

/// 备注指定计划的指定状态期次条数（多计划同批追补时定位失败计划用）。
#[then(expr = "备注为 {string} 的计划状态为 {string} 的期次应有 {int} 条")]
fn assert_occurrence_count_by_note(
    world: &mut LedgerWorld,
    note: String,
    status: String,
    expected: i64,
) {
    let count: i64 = {
        let conn = world_conn!(world);
        conn.query_row(
            "SELECT COUNT(*) FROM scheduled_transaction_occurrences o \
             JOIN scheduled_transactions s ON s.id = o.scheduled_transaction_id \
             WHERE s.note=?1 AND o.status=?2 AND o.is_deleted=0 AND s.is_deleted=0",
            params![note, status],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(
        count, expected,
        "计划「{note}」状态为 {status} 的期次条数不符"
    );
}
