use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tokio::sync::oneshot;
use tracing::Span;
use tracing::dispatcher::Dispatch;

use super::job_gate::{JobGate, LockOutcome, StartDecision};
use super::runtime::{
    DbState, SlotWatch, hold_threshold_for, probe_lock_hold_within, with_caller_context,
    write_locked,
};
use super::tx_scope::rollback_if_open;
use crate::error::{AppError, Result};

const WRITE_THREAD_NAME: &str = "db-write";

const READ_THREAD_NAME: &str = "db-read";

fn worker_gone_error(name: &str) -> AppError {
    AppError::Io(format!("数据库门面线程 {name} 已退出，作业结果丢失"))
}

type ErasedOutcome = Result<Box<dyn Any + Send>>;

struct Job {
    command: &'static str,
    dispatch: Dispatch,
    caller_span: Span,
    run: Box<dyn FnOnce(&Connection) -> ErasedOutcome + Send>,
    reply: Box<dyn FnOnce(ErasedOutcome) + Send>,
    gate: Arc<JobGate>,
    on_abandoned: Box<dyn FnOnce() -> ErasedOutcome + Send>,
}

enum WorkerMsg {
    Job(Job),
    Shutdown,
}

enum ConnectionTrust {
    Trusted,
    Untrusted(AppError),
}

impl ConnectionTrust {
    fn untrusted_error(&self) -> Option<AppError> {
        match self {
            ConnectionTrust::Trusted => None,
            ConnectionTrust::Untrusted(error) => Some(error.clone()),
        }
    }

    fn is_untrusted(&self) -> bool {
        matches!(self, ConnectionTrust::Untrusted(_))
    }

    fn reset_after_swap(&mut self) {
        if self.is_untrusted() {
            tracing::warn!(
                "连接槽已换连：新连接重置不可信标记（ADR-0125 决策 3 退役/重建，issue #1409）"
            );
        }
        *self = ConnectionTrust::Trusted;
    }

    fn mark_untrusted(&mut self, error: AppError) {
        if let ConnectionTrust::Untrusted(existing) = self {
            tracing::error!(
                existing = %existing,
                new = %error,
                "数据库连接已标记不可信，保留首个错误（后续作业报同一错误）"
            );
            return;
        }
        tracing::error!(error = %error, "数据库连接状态不可信：后续作业一律 fail-loud，不静默恢复");
        *self = ConnectionTrust::Untrusted(error);
    }
}

struct DbWorker {
    name: &'static str,
    sender: Mutex<Sender<WorkerMsg>>,
    handle: Option<JoinHandle<()>>,
}

impl DbWorker {
    fn spawn(name: &'static str, watch: SlotWatch) -> Result<DbWorker> {
        let (sender, receiver) = mpsc::channel::<WorkerMsg>();
        let handle = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || run_worker(name, watch, &receiver))
            .map_err(|e| AppError::Io(format!("数据库门面线程 {name} 启动失败: {e}")))?;
        Ok(DbWorker {
            name,
            sender: Mutex::new(sender),
            handle: Some(handle),
        })
    }

    fn submit<T, F>(
        &self,
        command: &'static str,
        execute: F,
    ) -> Result<oneshot::Receiver<Result<T>>>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel::<Result<T>>();
        self.enqueue(job_of(command, JobGate::queued(), execute, move |result| {
            let _ = reply_tx.send(result);
        }))?;
        Ok(reply_rx)
    }

    fn submit_blocking<T, F>(&self, command: &'static str, execute: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::channel::<Result<T>>();
        self.enqueue(job_of(command, JobGate::queued(), execute, move |result| {
            let _ = reply_tx.send(result);
        }))?;
        reply_rx.recv().map_err(|_| worker_gone_error(self.name))?
    }

    fn submit_blocking_within<T, F>(
        &self,
        command: &'static str,
        timeout: Duration,
        execute: F,
    ) -> LockOutcome<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::channel::<Result<T>>();
        let job_gate = JobGate::queued();
        if let Err(error) = self.enqueue(job_of(
            command,
            Arc::clone(&job_gate),
            execute,
            move |result| {
                let _ = reply_tx.send(result);
            },
        )) {
            return LockOutcome::Ran(Err(error));
        }
        match reply_rx.recv_timeout(timeout) {
            Ok(result) => LockOutcome::Ran(result),
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(result) = reply_rx.try_recv() {
                    return LockOutcome::Ran(result);
                }
                if job_gate.abandon() {
                    LockOutcome::Abandoned
                } else {
                    LockOutcome::Ran(
                        reply_rx
                            .recv()
                            .unwrap_or_else(|_| Err(worker_gone_error(self.name))),
                    )
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                LockOutcome::Ran(Err(worker_gone_error(self.name)))
            }
        }
    }

    fn enqueue(&self, job: Job) -> Result<()> {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sender
            .send(WorkerMsg::Job(job))
            .map_err(|_| AppError::Io(format!("数据库门面线程 {} 已关闭", self.name)))
    }

    fn shutdown(&mut self) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = sender.send(WorkerMsg::Shutdown);
        drop(sender);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    #[cfg(test)]
    fn is_finished(&self) -> bool {
        match &self.handle {
            None => true,
            Some(handle) => handle.is_finished(),
        }
    }
}

impl Drop for DbWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn job_of<T, F, R>(command: &'static str, gate: Arc<JobGate>, execute: F, reply: R) -> Job
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    R: FnOnce(Result<T>) + Send + 'static,
{
    Job {
        command,
        gate,
        dispatch: tracing::dispatcher::get_default(Dispatch::clone),
        caller_span: Span::current(),
        run: Box::new(move |conn| {
            let value = execute(conn)?;
            Ok(Box::new(value) as Box<dyn Any + Send>)
        }),
        reply: Box::new(move |outcome| {
            let result = match outcome {
                Ok(payload) => payload
                    .downcast::<T>()
                    .map(|value| *value)
                    .map_err(|_| AppError::Io("数据库门面作业回传载荷类型不匹配".into())),
                Err(error) => Err(error),
            };
            reply(result);
        }),
        on_abandoned: Box::new(|| Ok(Box::new(LockOutcome::<T>::Abandoned) as Box<dyn Any + Send>)),
    }
}

pub struct DbFacade {
    write: DbWorker,
    read: DbWorker,
}

impl DbFacade {
    pub fn start(state: &DbState) -> Result<DbFacade> {
        Self::start_slots(&state.conn, &state.read_conn)
    }

    pub(crate) fn start_slots(
        write: &Arc<Mutex<Connection>>,
        read: &Arc<Mutex<Connection>>,
    ) -> Result<DbFacade> {
        Ok(DbFacade {
            write: DbWorker::spawn(WRITE_THREAD_NAME, SlotWatch::new(write))?,
            read: DbWorker::spawn(READ_THREAD_NAME, SlotWatch::new(read))?,
        })
    }

    pub async fn run_write<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let receiver = self
            .write
            .submit(command, move |conn| write_locked(conn, f))?;
        await_reply(receiver).await
    }

    pub async fn run_write_raw<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let receiver = self.write.submit(command, f)?;
        await_reply(receiver).await
    }

    pub async fn run_read<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let receiver = self.read.submit(command, f)?;
        await_reply(receiver).await
    }

    pub fn run_write_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.write
            .submit_blocking(command, move |conn| write_locked(conn, f))
    }

    pub fn run_write_raw_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.write.submit_blocking(command, f)
    }

    pub fn run_write_raw_blocking_within<T, F>(
        &self,
        command: &'static str,
        timeout: Duration,
        f: F,
    ) -> LockOutcome<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.write.submit_blocking_within(command, timeout, f)
    }

    pub fn shutdown(&mut self) {
        self.write.shutdown();
        self.read.shutdown();
    }

    #[cfg(test)]
    pub(crate) fn workers_finished(&self) -> bool {
        self.write.is_finished() && self.read.is_finished()
    }
}

async fn await_reply<T>(receiver: oneshot::Receiver<Result<T>>) -> Result<T> {
    receiver
        .await
        .map_err(|_| AppError::Io("数据库门面作业结果丢失（DB 线程已退出）".into()))?
}

fn run_worker(name: &'static str, watch: SlotWatch, receiver: &mpsc::Receiver<WorkerMsg>) {
    let mut trust = ConnectionTrust::Trusted;
    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMsg::Shutdown => break,
            WorkerMsg::Job(job) => run_job(name, &watch, &mut trust, job),
        }
    }
}

fn run_job(name: &'static str, watch: &SlotWatch, trust: &mut ConnectionTrust, job: Job) {
    let Job {
        command,
        gate,
        dispatch,
        caller_span,
        run,
        reply,
        on_abandoned,
    } = job;
    if !gate.claim_for_run() {
        tracing::debug!(
            thread = name,
            command,
            "作业已在排队时被调用方弃权，本轮不执行"
        );
        reply(on_abandoned());
        return;
    }
    let started = Instant::now();
    let guard = match watch.slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let error = AppError::Db(format!("数据库连接锁已中毒: {poisoned}"));
            trust.mark_untrusted(error.clone());
            reply(Err(error));
            return;
        }
    };
    if gate.try_start() == StartDecision::Cancelled {
        tracing::debug!(
            thread = name,
            command,
            "作业在等槽期间被调用方弃权，本轮不执行"
        );
        reply(on_abandoned());
        return;
    }
    if watch.observe_swap() {
        trust.reset_after_swap();
    }
    if let Some(error) = trust.untrusted_error() {
        tracing::error!(thread = name, "数据库连接不可信，作业拒绝执行");
        reply(Err(error));
        return;
    }
    with_caller_context(&dispatch, &caller_span, command, || {
        let outcome = catch_unwind(AssertUnwindSafe(|| run(&guard)));
        let report = match outcome {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(error)) => Err(error),
            Err(panic) => Err(recover_from_panic(name, &guard, trust, &panic)),
        };
        probe_lock_hold_within(started.elapsed(), hold_threshold_for(command));
        drop(guard);
        reply(report);
    });
}

fn recover_from_panic(
    name: &'static str,
    conn: &Connection,
    trust: &mut ConnectionTrust,
    panic: &Box<dyn Any + Send>,
) -> AppError {
    let panic_error = panic_error(panic);
    match recover_after_panic(conn) {
        Ok(()) => {
            tracing::error!(
                thread = name,
                error = %panic_error,
                "数据库作业 panic：已回滚未提交事务，DB 线程继续服务"
            );
            panic_error
        }
        Err(recovery_error) => {
            trust.mark_untrusted(recovery_error.clone());
            recovery_error
        }
    }
}

fn recover_after_panic(conn: &Connection) -> Result<()> {
    #[cfg(test)]
    if force_recovery_failure_pending() {
        return Err(AppError::Db(
            "测试注入：作业 panic 后回滚失败，连接不可信".into(),
        ));
    }
    rollback_if_open(conn)
        .map_err(|e| AppError::Db(format!("作业 panic 后回滚失败，连接不可信: {e}")))
}

fn panic_error(payload: &Box<dyn Any + Send>) -> AppError {
    let detail = match payload.downcast_ref::<&'static str>() {
        Some(text) => (*text).to_string(),
        None => match payload.downcast_ref::<String>() {
            Some(text) => text.clone(),
            None => "非字符串载荷".to_string(),
        },
    };
    AppError::Io(format!("数据库任务执行失败: 作业 panic: {detail}"))
}

#[cfg(test)]
thread_local! {
    static FORCE_RECOVERY_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn force_next_recovery_failure() {
    FORCE_RECOVERY_FAILURE.with(|flag| flag.set(true));
}

#[cfg(test)]
fn force_recovery_failure_pending() -> bool {
    FORCE_RECOVERY_FAILURE.with(|flag| flag.replace(false))
}
