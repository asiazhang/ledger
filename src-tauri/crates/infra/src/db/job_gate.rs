//! 门面作业的**限时等待与放弃**原语（ADR-0125 决策 8 豁免台账退役，issue #1415）：
//! 调用方按时限等作业结果，等不到就放弃本轮；弃权成立时作业**不执行**——不管它
//! 还在队列里排队，还是已经由 DB 线程认领、正等着连接槽。反过来，调用方的弃权若
//! 晚于作业开始执行，弃权不生效，双方照常等结果（执行中不可撤销，ADR-0125 否决段
//! 「不引半成品取消语义」）。
//!
//! **为什么需要它**：决策 8 的豁免台账里，备份自动调度与追补的语义是「取不到连接
//! 就放弃本轮、下个周期重试」——排队式门面（只有一个作业通道、无时限）给不出等价
//! 物，故当时留在台账。本原语补上这一格。
//!
//! **弃权窗口覆盖「等槽」这一整段**：DB 线程的一条作业若无可执行空间，就阻塞在
//! 连接槽锁上；此时调用方的时限早就过了，若把「认领」理解成「作业开始执行」，
//! 调用方就得陪它等到槽释放——那正是要消灭的「等到底」。故作业的状态机里有一格
//! [`JobState::Waiting`]（已认领、在等槽、**作业体尚未开始**），调用方的弃权在
//! 这一格同样成立；DB 线程拿到槽后先裁决门，弃权已成立就跳过作业体、只还槽。

use std::sync::{Arc, Mutex};

/// 作业状态机：`Queued`（已入队）→ `Waiting`（DB 线程认领、等连接槽，作业体未开始）
/// → `Running`（作业体执行中）；`Abandoned` 是调用方在**作业体开始前**弃权（终态，
/// 作业体不得执行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobState {
    /// 已入队、DB 线程尚未认领。此时弃权成立。
    Queued,
    /// 已认领、正等连接槽（作业体尚未开始）。此时弃权仍成立——「等槽」本来就属
    /// 于调用方的限时窗口。
    Waiting,
    /// 作业体已开始执行：弃权不再生效（执行中不可撤销，ADR-0125 否决段）。
    Running,
    /// 调用方已弃权本轮：作业体不执行。
    Abandoned,
}

/// DB 线程拿到连接槽后的执行裁决（[`JobGate::try_start`] 的返回值）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartDecision {
    /// 本轮执行作业体（门转 `Running`）。
    Run,
    /// 调用方已在等待期间弃权：作业体不执行（门已是终态 `Abandoned`）。
    Cancelled,
}

/// 限时等待作业的状态门（同一把锁下裁决「弃权」与「开始执行」）。
#[derive(Debug)]
pub(crate) struct JobGate {
    state: Mutex<JobState>,
}

impl JobGate {
    /// 新建已入队的门（调用方持 [`Arc`]：作业进队时随作业一并交给门面）。
    pub(crate) fn queued() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(JobState::Queued),
        })
    }

    /// DB 线程从队列取到作业时认领（作业仍在 `Queued` 时成立）：门转 `Waiting`，
    /// 表示「作业体尚未开始、正在等连接槽」。
    ///
    /// 返回 `false` 表示调用方已在排队期间弃权——作业体不得执行。
    pub(crate) fn claim_for_run(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *state == JobState::Queued {
            *state = JobState::Waiting;
            true
        } else {
            false
        }
    }

    /// DB 线程拿到连接槽后的裁决：弃权先到（或已在终态）则作业体不执行。
    ///
    /// 与 [`JobGate::abandon`] 共用同一把锁，故「弃权」与「开始执行」互斥且可判别
    /// ——不存在「弃权已成立却仍执行作业体」的窗口。
    pub(crate) fn try_start(&self) -> StartDecision {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *state {
            JobState::Waiting => {
                *state = JobState::Running;
                StartDecision::Run
            }
            JobState::Queued | JobState::Running | JobState::Abandoned => StartDecision::Cancelled,
        }
    }

    /// 调用方在时限到达时弃权本轮：作业体尚未开始时成立（门转终态 `Abandoned`），
    /// 已开始执行时返回 `false`——弃权不生效，调用方应继续等结果。
    pub(crate) fn abandon(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *state {
            JobState::Queued | JobState::Waiting => {
                *state = JobState::Abandoned;
                true
            }
            JobState::Running | JobState::Abandoned => false,
        }
    }
}

/// 限时等待作业的结果（ADR-0125 决策 8 豁免台账退役，issue #1415）：
/// 区分「本轮作业执行了」与「调用方弃权、作业体未执行」——排队式门面本身给不出
/// 这个判别，而「取不到就放弃本轮、下个周期重试」的语义正需要它。
#[derive(Debug)]
pub enum LockOutcome<T> {
    /// 作业已执行（结果或业务错误见内层）。
    Ran(crate::error::Result<T>),
    /// 调用方在时限内等不到作业开始执行：本轮放弃，作业体零执行、零副作用
    /// （下个周期重试是调用方的语义）。
    Abandoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 排队中弃权成立：作业体不得执行（认领失败）。
    #[test]
    fn abandon_while_queued_cancels_before_claim() {
        let gate = JobGate::queued();
        assert!(gate.abandon(), "排队中的作业应可被弃权");
        assert!(!gate.claim_for_run(), "已弃权的作业不得被认领");
    }

    /// 认领后仍在等槽：弃权依旧成立，且拿到槽时裁决为「不执行」。
    #[test]
    fn abandon_while_waiting_cancels_before_body_runs() {
        let gate = JobGate::queued();
        assert!(gate.claim_for_run(), "排队中的作业应可被认领");
        assert!(gate.abandon(), "等槽阶段仍属调用方的限时窗口，弃权应成立");
        assert_eq!(
            gate.try_start(),
            StartDecision::Cancelled,
            "弃权已成立时作业体不得执行"
        );
    }

    /// 开始执行先到：弃权不再生效（执行中不可撤销）。
    #[test]
    fn start_wins_over_later_abandon() {
        let gate = JobGate::queued();
        assert!(gate.claim_for_run(), "排队中的作业应可被认领");
        assert_eq!(gate.try_start(), StartDecision::Run, "等槽结束后应裁决执行");
        assert!(!gate.abandon(), "作业体已开始，调用方的弃权请求不生效");
        assert_eq!(
            gate.try_start(),
            StartDecision::Cancelled,
            "同一作业不得被裁决执行两次"
        );
    }
}
