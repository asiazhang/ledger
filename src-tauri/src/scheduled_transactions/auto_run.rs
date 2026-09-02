//! 定时计划自动执行（追补，issue #307 / ADR-0042）。
//!
//! 职责边界：
//! - 设备级「自动执行」开关的真源是前端 localStorage 设备偏好（ADR-0017 边界：
//!   刻意不入 `app_settings`——该表随 Backup/Restore 迁移，表达不了「这台执行、
//!   那台不执行」），后端只持一份进程级运行时镜像 [`ENABLED`]（默认关），
//!   由领域命令形状的推送命令（`commands::scheduled`）在应用启动与变更时更新；
//! - 追补入口 [`run_catch_up`] 是唯一新增接缝：参数注入（连接、开关状态、今天
//!   日期）→ 执行汇总，所有后端行为测试打这一个入口；期次执行本体沿用引擎既有
//!   单期执行入口（事务自持 + CAS）零改动；
//! - 调度归并在自动备份轮询线程的单一 tick：每轮先备份到期判定、再追补判定
//!   （ADR-0016 修订注记的 10 分钟周期），线程只做周期调用——开关由线程从镜像
//!   读出后注入，决策全在本入口。
//!
//! 追补语义（ADR-0042）：到期 = 期次 `pending` 且计划日期 ≤ 今天（含今天），且
//! 计划 `active`；生成交易日期忠实回填期次计划日期。`failed` / `processing` /
//! `cancelled` 期次与 `paused` / `cancelled` 计划一律不碰；单期尝试失败置为
//! `failed` 保持手动重试（ADR-0024 失败策略维持），不自动反复重试；单期失败不
//! 中断同批后续；每笔成功经统一写入口语义置脏（[`crate::backup::mark_dirty`]）
//! 联动自动备份到期判定。

use std::sync::atomic::{AtomicBool, Ordering};

use chrono::NaiveDate;
use rusqlite::{Connection, params};

use super::engine::execute_occurrence;
use crate::db::{device_id, now_iso};
use crate::error::Result;

/// 后端运行时镜像（进程级）：设备级开关默认关，前端启动/变更时经 IPC 推送更新。
static ENABLED: AtomicBool = AtomicBool::new(false);

/// 推送开关到运行时镜像（推送命令的唯一写点）。
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
    tracing::info!(enabled, "自动执行开关镜像已更新");
}

/// 读取镜像（调度线程每轮读出后注入追补入口）。
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::SeqCst)
}

/// 一次追补的执行汇总。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CatchUpSummary {
    /// 本轮扫到的到期期次数（开关关闭时恒为 0——空转不查询）。
    pub due: usize,
    /// 成功落账的期次数。
    pub executed: usize,
    /// 尝试失败的期次数（单期失败不中断后续；失败期次已置 failed 待手动重试）。
    pub failed: usize,
    /// 失败明细（期次 id → 错误信息），落日志与测试断言用。
    pub failures: Vec<(String, String)>,
}

/// 追补入口（唯一新增接缝，ADR-0042）：把到期期次（`pending` 且计划日期 ≤
/// `today`、计划 `active`）逐条经引擎既有单期执行入口自动落账，交易日期忠实取
/// 期次计划日期。开关关闭时空转（不查询、不动任何期次）；单期失败不中断后续，
/// 失败期次 CAS 置 `failed` 保持手动重试。执行汇总随返回值交出并落日志。
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
                // 与连接层统一写入口提交点同款：成功落账即置脏（联动自动备份判定）。
                // 置脏失败仅记日志不上抛，不影响已成功的落账。
                if let Err(e) = crate::backup::mark_dirty(conn) {
                    tracing::warn!(
                        occurrence_id = %occurrence_id,
                        error = %e,
                        "追补落账成功但置脏失败（忽略）"
                    );
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
        // 空转/无动作轮（含开关关闭）：每 10 分钟一轮，只落 debug 避免刷屏。
        tracing::debug!(due = summary.due, enabled, "定时计划自动执行追补本轮无动作");
    }
    summary
}

/// 到期期次清单：`pending` 且计划日期 ≤ 今天，且所属计划 `active`；按计划日期
/// 升序追补（先到期先落账）。
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

/// 把尝试失败的期次 CAS 置为 `failed`（仅当仍为 `pending`：并发下已被其他设备
/// 推进的期次不被本设备改写），保持既有单期执行命令的手动重试入口。
fn mark_failed(conn: &Connection, occurrence_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_transaction_occurrences SET status='failed', updated_at=?2, \
         version=version+1, device_id=?3 \
         WHERE id=?1 AND status='pending' AND is_deleted=0",
        params![occurrence_id, now_iso(), device_id()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 运行时镜像默认关（验收⑤：后端运行时镜像默认关）：未推送即关，
    /// 推送命令可翻转并可复位。追补行为测试不消费镜像（开关一律注入），
    /// 故进程级 static 在并行测试间无干扰。
    #[test]
    fn mirror_defaults_off_and_push_flips() {
        assert!(!is_enabled(), "镜像默认关");
        set_enabled(true);
        assert!(is_enabled(), "推送开启后应翻转");
        set_enabled(false);
        assert!(!is_enabled(), "推送关闭后应复位");
    }
}
