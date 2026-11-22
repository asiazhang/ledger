use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobState {
    Queued,
    Waiting,
    Running,
    Abandoned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartDecision {
    Run,
    Cancelled,
}

#[derive(Debug)]
pub(crate) struct JobGate {
    state: Mutex<JobState>,
}

impl JobGate {
    pub(crate) fn queued() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(JobState::Queued),
        })
    }

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

#[derive(Debug)]
pub enum LockOutcome<T> {
    Ran(crate::error::Result<T>),
    Abandoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abandon_while_queued_cancels_before_claim() {
        let gate = JobGate::queued();
        assert!(gate.abandon(), "排队中的作业应可被弃权");
        assert!(!gate.claim_for_run(), "已弃权的作业不得被认领");
    }

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
