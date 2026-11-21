use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{Local, NaiveDate};
use rusqlite::{Connection, params};

use super::engine::execute_occurrence;
use ledger_infra::db::now_iso;
use ledger_infra::error::Result;
use ledger_sync_protocol::device::device_id;

type AfterOccurrenceHook = fn(&Connection);

static AFTER_OCCURRENCE_HOOK: OnceLock<AfterOccurrenceHook> = OnceLock::new();

pub fn register_after_occurrence_hook(hook: AfterOccurrenceHook) {
    let _ = AFTER_OCCURRENCE_HOOK.set(hook);
}

pub fn catch_up_hook(conn: &Connection) {
    run_catch_up(conn, is_enabled(), Local::now().date_naive());
}

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
    tracing::info!(enabled, "自动执行开关镜像已更新");
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatchUpSummary {
    pub due: usize,
    pub executed: usize,
    pub failed: usize,
    pub failures: Vec<(String, String)>,
}

pub fn run_catch_up(conn: &Connection, enabled: bool, today: NaiveDate) -> CatchUpSummary {
    let mut summary = CatchUpSummary::default();
    if !enabled {
        return summary;
    }
    let today = today.format("%Y-%m-%d").to_string();
    let due_ids = match due_occurrence_ids(conn, &today) {
        Ok(ids) => ids,
        Err(e) => {
            tracing::error!(error = %e, "追补到期期次查询失败，本轮跳过");
            return summary;
        }
    };
    summary.due = due_ids.len();
    for occurrence_id in due_ids {
        match execute_occurrence(conn, &occurrence_id) {
            Ok(_) => {
                summary.executed += 1;
                match AFTER_OCCURRENCE_HOOK.get() {
                    Some(hook) => hook(conn),
                    None => tracing::error!(
                        occurrence_id = %occurrence_id,
                        "期次落账后置钩子未注册：置脏被跳过（壳层启动接线缺失）"
                    ),
                }
            }
            Err(e) => {
                summary.failed += 1;
                summary
                    .failures
                    .push((occurrence_id.clone(), e.to_string()));
                tracing::warn!(
                    occurrence_id = %occurrence_id,
                    error = %e,
                    "自动执行期次失败，置为 failed 保持手动重试"
                );
                if let Err(mark_err) = mark_failed(conn, &occurrence_id) {
                    tracing::warn!(
                        occurrence_id = %occurrence_id,
                        error = %mark_err,
                        "失败期次标记 failed 失败（下轮 CAS 守卫重试标记）"
                    );
                }
            }
        }
    }
    if summary.executed > 0 || summary.failed > 0 {
        tracing::info!(
            due = summary.due,
            executed = summary.executed,
            failed = summary.failed,
            "定时计划自动执行追补完成"
        );
    } else {
        tracing::debug!(due = summary.due, enabled, "定时计划自动执行追补本轮无动作");
    }
    summary
}

fn due_occurrence_ids(conn: &Connection, today: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT o.id FROM scheduled_transaction_occurrences o \
         JOIN scheduled_transactions s ON s.id = o.scheduled_transaction_id \
         WHERE o.status='pending' AND o.is_deleted=0 AND o.scheduled_date <= ?1 \
           AND s.status='active' AND s.is_deleted=0 \
         ORDER BY o.scheduled_date ASC",
    )?;
    let ids = stmt
        .query_map(params![today], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn mark_failed(conn: &Connection, occurrence_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status='failed', updated_at=?2, \
         version=version+1, device_id=?3 \
         WHERE id=?1 AND status='pending' AND is_deleted=0",
        params![occurrence_id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_defaults_off_and_push_flips() {
        assert!(!is_enabled(), "镜像默认关");
        set_enabled(true);
        assert!(is_enabled(), "推送开启后应翻转");
        set_enabled(false);
        assert!(!is_enabled(), "推送关闭后应复位");
    }
}
