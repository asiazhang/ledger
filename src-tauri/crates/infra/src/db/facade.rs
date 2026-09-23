//! 异步 DB 门面（ADR-0125 决策 1–3，spec #1406 / issue #1408）：写 / 读两条专用
//! DB 线程 + 作业通道 + oneshot 回传。
//!
//! **为什么有这一层**：DB 闭包今天由 tokio 阻塞线程池执行（`db::run_db`，
//! ADR-0069 形状乙），连接槽锁由各调用点自己取——线程归属靠「调用点写对」的隐性
//! 纪律维持，纪律退化以 panic 或卡死暴露（#1403 现场）。门面把取用独占收进 DB
//! 线程：调用方 `await` 作业结果，连接的生老病死都在 DB 线程上。
//!
//! - **两条线程、各一条连接**（决策 1）：写线程持写槽、读线程持读槽。读写不共线程
//!   ——否则长写事务会把读排在后面，正是 ADR-0117 修掉的退化。连接槽与成对构造点
//!   保持原形（`DbState` 不动），门面只为整个作业持有槽锁并把 `&Connection` 交给
//!   作业闭包：闭包内再取同一槽即自死锁（越界形态由「已持锁」写入口与结构守门挡住）；
//! - **契约承接**（决策 2）：写作业在 DB 线程上经连接层统一写入口的「已持锁」形态
//!   `write_locked` 执行——提交点后置动作（置脏 + 写时到期检查）与 autocommit 复核
//!   仍是同一条结构，只是触发者变成 DB 线程；作业占用 DB 线程时长照走
//!   `probe_lock_hold`（阈值与告警口径不变）；调用点 dispatcher 与 span 跨线程
//!   显式带入（与 `db::run_db` 同款），SQL 归因不漂移；
//! - **换连承接**（决策 2，issue #1409）：换连仍走 ADR-0117 决策 3 的成对原语
//!   （不改门面作业），门面线程与换连原语共用连接槽互斥体，故换连与在途作业互斥、
//!   对换连后的作业立即可见；换连原语在槽锁内抬高「换连代次」，DB 线程取锁后比对
//!   代次即识别换连并复位连接不可信标记（决策 3 的退役 / 重建）；
//! - **panic 语义**（决策 3）：`catch_unwind` 位于连接槽守卫作用域**之内**——panic
//!   在守卫释放前被拦下，互斥体不中毒；随后回滚未提交事务（`tx_scope::rollback_if_open`）、
//!   错误经通道 fail-loud 上报，DB 线程不死也不静默重启。回滚失败或连接状态不可信
//!   （含槽锁已中毒）时标记该连接，后续作业一律 fail-loud 报同一错误，不静默恢复。
//!
//! 门面的启动与关闭：`DbFacade::start` 拉起两条线程，`DbFacade::shutdown`（或
//! `Drop`）投停机消息并 join——在途作业跑完为止，不打断。
//!
//! 测试见 `db/tests/facade.rs`；调用方改道（三处统一入口与命令层直锁）随 #1410。

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

/// 写 DB 线程名（线程名是可观察坐标：panic 现场与采样归因按名定位）。
const WRITE_THREAD_NAME: &str = "db-write";

/// 读 DB 线程名（语义同 `WRITE_THREAD_NAME`）。
const READ_THREAD_NAME: &str = "db-read";

/// 作业结果丢失的错误载荷（issue #1415 收为单点）：回传通道先于结果断开，只可能是
/// DB 线程已退出（停机或装配失败）——fail-loud，不静默当作成功。
fn worker_gone_error(name: &str) -> AppError {
    AppError::Io(format!("数据库门面线程 {name} 已退出，作业结果丢失"))
}

/// 作业结果（类型擦除）：Ok 侧是业务值装箱、Err 侧是原样传播的错误。
type ErasedOutcome = Result<Box<dyn Any + Send>>;

/// 门面作业：DB 线程在连接槽守卫内执行 `Job::run`，再经 `Job::reply` 回传。
///
/// 两段拆开是为让「作业体」（调用方闭包，类型 T 在 `DbWorker::submit` 内擦除）
/// 与「结果回传」分离：作业体 panic 被门面拦下后，回传仍由门面侧代发 fail-loud
/// 错误（回传通道不随 panic 一起丢）。
struct Job {
    /// SQL 归因串（IPC 命令名 / HTTP 端点键，语义同 `db::run_db` 的 `command`）。
    command: &'static str,
    /// 调用点 dispatcher：线程局部，跨线程显式带入（ADR-0069 决策 3）。
    dispatch: Dispatch,
    /// 调用点当前 span；为空时门面侧重建 `command` span 兜底（IPC 异步命令形态）。
    caller_span: Span,
    /// 作业体：拿已持锁连接，返回类型擦除结果（业务错误原样上抛）。
    run: Box<dyn FnOnce(&Connection) -> ErasedOutcome + Send>,
    /// 结果回传（oneshot 发送端装箱，供门面侧在 panic 后仍能回传错误）。
    reply: Box<dyn FnOnce(ErasedOutcome) + Send>,
    /// 状态门（限时等待与放弃原语，issue #1415）：DB 线程开始执行前在此认领，
    /// 调用方弃权时在此放弃——两者共用同一把锁，裁决互斥且可判别。
    gate: Arc<JobGate>,
    /// 放弃时的回传载荷（issue #1415）：作业体未执行时门面回传
    /// [`LockOutcome::Abandoned`]；载荷随作业泛型 T 就地构造（类型擦除后回传
    /// 与作业结果的拆箱同一上下文）。
    on_abandoned: Box<dyn FnOnce() -> ErasedOutcome + Send>,
}

/// 门面线程收到的消息。
enum WorkerMsg {
    Job(Job),
    Shutdown,
}

/// 连接可信性（ADR-0125 决策 3）：一旦不可信，后续作业一律 fail-loud 报同一错误，
/// 不静默恢复服务。
///
/// **标记的生命周期 = 门面线程的生命周期**（本票现场判定）：连接槽与成对构造点受
/// ADR-0125 决策 4 保护（测试世界零改造，`DbState` 不加字段），故标记不挂连接槽。
/// 生产接线是进程级单例门面（#1410），单例生命周期内「标记后不静默恢复」成立；槽锁
/// 中毒这一诱发因本身持久（同槽再锁仍中毒），故重起门面也不会静默服务它。
///
/// **换连即复位**（ADR-0125 决策 3 把「退役 / 重建该连接」交给实施票，issue #1409
/// 判定为复位）：标记是**连接**级的，换连换进来的是另一条连接，旧连接的失败态不
/// 继承。识别手段是换连原语在槽锁内抬高的「换连代次」（`db::runtime::slot_swap_epoch`），
/// 复位发生在作业取到槽锁之后、不可信判定之前。非换连路径上的失败态仍持续——复位
/// 只认「槽被换连」这一事实，不认「槽当下能不能锁上」，不静默恢复服务。
enum ConnectionTrust {
    Trusted,
    Untrusted(AppError),
}

impl ConnectionTrust {
    /// 已标记的不可信错误（可信时为 `None`）。
    fn untrusted_error(&self) -> Option<AppError> {
        match self {
            ConnectionTrust::Trusted => None,
            ConnectionTrust::Untrusted(error) => Some(error.clone()),
        }
    }

    /// 是否已标记不可信。
    fn is_untrusted(&self) -> bool {
        matches!(self, ConnectionTrust::Untrusted(_))
    }

    /// 换连后复位（ADR-0125 决策 3 的「退役 / 重建」由实施票判定，issue #1409）：
    /// 槽内连接已被换连原语换成另一条连接，旧连接的失败态不继承。只有确实清掉了
    /// 标记才留痕——「服务从拒答恢复」是可观察事件，按警告级别记，不静默。
    fn reset_after_swap(&mut self) {
        if self.is_untrusted() {
            tracing::warn!(
                "连接槽已换连：新连接重置不可信标记（ADR-0125 决策 3 退役/重建，issue #1409）"
            );
        }
        *self = ConnectionTrust::Trusted;
    }

    /// 标记连接不可信（记错误日志、保留首个错误——「报同一错误」的载体）。
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

/// 单条 DB 线程：作业通道的持有方 + 线程收尾句柄。
struct DbWorker {
    /// 线程名（停机与失败日志的归因坐标）。
    name: &'static str,
    /// 作业通道发送端；全部发送端释放即线程自然退出（与显式停机等价）。
    ///
    /// `Mutex` 包裹的理由是 `Sender` 本身非 `Sync`，而门面句柄要能被壳层应用状态
    /// 持有或被 `Arc` 共享（#1410 接线）——锁只覆盖「一次入队」，不覆盖作业执行，
    /// 因此不引入第二处排队（作业仍只在 DB 线程上串行）。
    sender: Mutex<Sender<WorkerMsg>>,
    /// 线程句柄；`shutdown` 取走后置 `None`。
    handle: Option<JoinHandle<()>>,
}

impl DbWorker {
    /// 拉起一条 DB 线程，接管给定连接槽的共享句柄。
    ///
    /// `watch` 是槽的观测句柄（issue #1409）：DB 线程靠它在取到槽锁后识别「槽已被
    /// 换连」，据此复位连接不可信标记。
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

    /// 投递作业并拿到结果接收端（不等待作业执行）。
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
            // 调用方已放弃等待（future 被 drop）时静默丢弃——不是失败。
            let _ = reply_tx.send(result);
        }))?;
        Ok(reply_rx)
    }

    /// 投递作业并**阻塞等待**结果（分段写入口的每一段取连接，issue #1410）：
    /// 调用方在阻塞线程上（分段编排体），std 通道等待不引入第二套异步形态，
    /// 也不要求调用点在 tokio 运行时内。
    fn submit_blocking<T, F>(&self, command: &'static str, execute: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::channel::<Result<T>>();
        self.enqueue(job_of(command, JobGate::queued(), execute, move |result| {
            // 接收端已放弃等待时静默丢弃——不是失败。
            let _ = reply_tx.send(result);
        }))?;
        reply_rx.recv().map_err(|_| worker_gone_error(self.name))?
    }

    /// 限时等待的阻塞提交（ADR-0125 决策 8 豁免台账退役，issue #1415）：语义同
    /// [`DbWorker::submit_blocking`]，但调用方最多等 `timeout`——超时即放弃本轮，
    /// 返回 [`LockOutcome::Abandoned`]。
    ///
    /// **放弃的裁决点在队列侧**：超时时调用方在状态门上请求放弃（[`JobGate::abandon`]），
    /// 作业体尚未开始（排队中或 DB 线程正等槽）就**不会执行**（零副作用，
    /// `Abandoned`），已开始执行则跑到出结果（执行中不可撤销，ADR-0125 否决段）
    /// ——两种结果互斥且可判别，「取不到就放弃本轮、下个周期重试」的语义因此有
    /// 等价物。等槽之所以也落在弃权窗口内：调用方的时限本就是「等到连接可用」，
    /// 把窗口截在开头会退化成没有时限的等待。
    ///
    /// 结果与放弃的竞速按接收通道裁决：时限到达时结果**已经就绪**（作业已完成）
    /// 优先取结果——放弃只应对「还没做完」的等待生效。
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
                // 接收端已放弃等待时静默丢弃——不是失败。
                let _ = reply_tx.send(result);
            },
        )) {
            return LockOutcome::Ran(Err(error));
        }
        match reply_rx.recv_timeout(timeout) {
            Ok(result) => LockOutcome::Ran(result),
            Err(RecvTimeoutError::Timeout) => {
                // 结果已经就绪（作业已完成）优先取结果：放弃只对「还没做完」的等待
                // 生效，否则时限恰好落在完成瞬间会误报「本轮没跑」。
                if let Ok(result) = reply_rx.try_recv() {
                    return LockOutcome::Ran(result);
                }
                // 作业体尚未开始（排队中，或 DB 线程正等槽）→ 弃权成立、本轮不执行；
                // 已开始执行 → 弃权不生效（ADR-0125 否决段），等它出结果。
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

    /// 入队一条作业（锁内只有一次入队、没有可毒化的不变量：中毒——理论上不可达
    /// ——按原样取用，失败判定只看通道本身是否已关闭）。
    fn enqueue(&self, job: Job) -> Result<()> {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sender
            .send(WorkerMsg::Job(job))
            .map_err(|_| AppError::Io(format!("数据库门面线程 {} 已关闭", self.name)))
    }

    /// 停机并等线程退出（幂等）：投停机消息后 join——在途作业跑完才返回，不打断。
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

    /// 线程是否已退出（收尾观察点，测试构建可见）。
    #[cfg(test)]
    fn is_finished(&self) -> bool {
        match &self.handle {
            // 句柄已被 `shutdown` 取走并 join 成功 → 线程确已退出。
            None => true,
            Some(handle) => handle.is_finished(),
        }
    }
}

impl Drop for DbWorker {
    /// 句柄释放即线程收尾（与 [`DbFacade::shutdown`] 同义）：投停机消息并 join。
    /// 门面句柄与门面局部装配（如第二条线程启动失败时已被拉起的另一条）都经此收尾，
    /// 不留脱管线程。
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// 组装一条作业（issue #1410 抽为单点：异步 oneshot 回传与阻塞 std 通道回传
/// 只在「回传通道」上分叉，作业体与上下文携带逐字同源）。状态门由调用方建
/// （issue #1415：限时等待路径要拿它来弃权本轮），普通路径就地新建一枚——
/// 无时限的调用方从不弃权，门恒走到 `Running`。
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
                // 装箱 / 拆箱在同一泛型上下文内成对，类型不符即门面内部缺陷。
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

/// 异步 DB 门面（ADR-0125 决策 1）：写作业与读作业各走一条专用 DB 线程。
///
/// 消费面（issue #1410）：三处统一入口与命令层 / 壳层直锁改道点拿**类型化句柄**
/// （`facade_handles` 的读 / 写句柄，由 `DbState` / HTTP 壳状态的具名访问器产出）
/// 投作业——句柄按写槽解析到本类型，生产面由引导期安装的进程级单例
/// （`facade_handles::install_facade`）常驻，测试世界按槽惰性拉起（`facade_for`）。
/// 本类型自身也可直接消费（领域/基础设施内部装配与 `db/tests/facade.rs`）——
/// `DbFacade::start` 只在装配处调一次，句柄生命周期内线程常驻；类型
/// `Send + Sync`，可住进壳层应用状态或被 `Arc` 共享（测试
/// `facade_handle_is_send_and_sync` 钉住该性质）。门面句柄释放即两条线程收尾
/// （字段析构走 `DbWorker` 的 `Drop`）。
pub struct DbFacade {
    /// 写 DB 线程（持写槽；提交点后置动作由它触发）。
    write: DbWorker,
    /// 读 DB 线程（持只读读槽；与写侧独立，ADR-0117）。
    read: DbWorker,
}

impl DbFacade {
    /// 启动门面：拉起写 / 读两条 DB 线程，各接管既有连接槽的共享句柄。
    ///
    /// 连接槽与成对构造点保持原形（`DbState` 不动）——测试世界与调用面的构造
    /// 零改造（ADR-0125 决策 4）。线程启动失败如实上抛（fail loud，不静默退化为
    /// 调用线程内联执行）。
    pub fn start(state: &DbState) -> Result<DbFacade> {
        Self::start_slots(&state.conn, &state.read_conn)
    }

    /// 按槽对启动门面（`DbFacade::start` 与按槽解析单点 `facade_for` 共用）：
    /// 句柄形态（`super::facade_handles` 的读 / 写句柄）按槽对解析门面，故启动
    /// 只认槽、不认状态的持有者。
    pub(crate) fn start_slots(
        write: &Arc<Mutex<Connection>>,
        read: &Arc<Mutex<Connection>>,
    ) -> Result<DbFacade> {
        Ok(DbFacade {
            write: DbWorker::spawn(WRITE_THREAD_NAME, SlotWatch::new(write))?,
            read: DbWorker::spawn(READ_THREAD_NAME, SlotWatch::new(read))?,
        })
    }

    /// 写作业（ADR-0125 决策 2）：作业在写 DB 线程上经连接层统一写入口的「已持锁」
    /// 形态执行——提交点后置动作（置脏 + 写时到期检查）与 autocommit 复核语义与
    /// 取锁形态同源（`write_locked` 是唯一实现）。
    ///
    /// 读形态但闭包内含惰性写的命令（缓存自愈一类，ADR-0117 已逐命令留痕）走本
    /// 入口；本裁决不重开甄别。
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

    /// 写槽裸作业（issue #1410）：作业在写 DB 线程上执行，但**不经**连接层统一
    /// 写入口——分段形态的每一段取连接即本形态（逐段 autocommit，置脏与信号由
    /// 分段入口在收尾裁决点恰好一次触发，ADR-0073 / #1276/#1277 语义不变）。
    pub async fn run_write_raw<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let receiver = self.write.submit(command, f)?;
        await_reply(receiver).await
    }

    /// 读作业（ADR-0125 决策 1）：作业在读 DB 线程上执行，不触碰置脏维度（读路径
    /// 无写后置动作，ADR-0104）；读侧与写侧各持一线程，长写事务不把读排在后面
    /// （ADR-0117 语义原样）。
    pub async fn run_read<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let receiver = self.read.submit(command, f)?;
        await_reply(receiver).await
    }

    /// 写作业的**阻塞等待**形态（分段写入口的每一段取连接，issue #1410）：语义与
    /// [`DbFacade::run_write`] 逐字一致，只是调用方在阻塞线程上按 std 通道等待。
    pub fn run_write_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.write
            .submit_blocking(command, move |conn| write_locked(conn, f))
    }

    /// 写槽裸作业的阻塞等待形态（分段形态的每一段取连接）：语义与
    /// [`DbFacade::run_write_raw`] 逐字一致，只是调用方在阻塞线程上等待。
    pub fn run_write_raw_blocking<T, F>(&self, command: &'static str, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        self.write.submit_blocking(command, f)
    }

    /// 写槽裸作业的**限时等待**形态（ADR-0125 决策 8 豁免台账退役，issue #1415）：
    /// 语义与 [`DbFacade::run_write_raw_blocking`] 逐字一致，只是调用方最多等
    /// `timeout`——等不到作业开始执行就放弃本轮（返回
    /// [`LockOutcome::Abandoned`]，作业体零执行），已在途则等它出结果。
    ///
    /// 消费面：备份自动调度与追补、退出兜底与壳层首次兜底（「取不到连接就放弃
    /// 本轮、下个周期重试」，ADR-0125 决策 8 豁免台账退役，issue #1415）。多端
    /// 同步调度侧的轮次取连接**尚未**走本入口（`RoundConn` 接缝闭包借用轮次现场、
    /// 不满足作业要求的 `Send + 'static`，仍走 `ledger_backup::lock_conn_with_timeout`
    /// 的直锁形态），收编随多端同步域异步化另案（#1405）一并评估。
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

    /// 显式停机：投停机消息并等待两条 DB 线程退出（在途作业跑完为止）。未显式调用
    /// 时由 `Drop` 承担同一语义；停机后投递作业 fail-loud 报错，不静默丢弃。
    pub fn shutdown(&mut self) {
        self.write.shutdown();
        self.read.shutdown();
    }

    /// 收尾观察点：两条 DB 线程是否都已退出（`shutdown` / `Drop` 之后成立，
    /// 测试构建可见）。
    #[cfg(test)]
    pub(crate) fn workers_finished(&self) -> bool {
        self.write.is_finished() && self.read.is_finished()
    }
}

/// 等作业结果：接收端被丢弃（作业未回传就退出）即 fail-loud，不静默当作成功。
async fn await_reply<T>(receiver: oneshot::Receiver<Result<T>>) -> Result<T> {
    receiver
        .await
        .map_err(|_| AppError::Io("数据库门面作业结果丢失（DB 线程已退出）".into()))?
}

/// DB 线程主循环：逐条消费作业，直到停机消息或全部发送端释放。
fn run_worker(name: &'static str, watch: SlotWatch, receiver: &mpsc::Receiver<WorkerMsg>) {
    let mut trust = ConnectionTrust::Trusted;
    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMsg::Shutdown => break,
            WorkerMsg::Job(job) => run_job(name, &watch, &mut trust, job),
        }
    }
}

/// 执行一条作业：取连接槽锁 → 守卫作用域内执行（含 panic 拦截）→ 回传结果。
///
/// 取锁之后先比对换连代次（issue #1409）：换连原语在槽锁内抬高代次，故本处读到
/// 的值要么已含换连、要么不含——「槽已换连」即复位连接不可信标记（ADR-0125
/// 决策 3：退役 / 重建由实施票判定），不可信判定随后按（可能已复位）的标记走。
/// 代次比对必须在槽锁内，锁外读会与换连竞速（读到旧代次却用上了新连接）。
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
    // 限时等待的两段裁决（issue #1415）：认领时调用方已弃权（排队中超时）→ 作业体
    // 不执行、槽锁都不必取；认领成功则转「等槽」态——这一步之后调用方的弃权仍
    // 成立（等槽同属调用方的限时窗口），故拿到槽后还要再裁决一次。
    if !gate.claim_for_run() {
        tracing::debug!(
            thread = name,
            command,
            "作业已在排队时被调用方弃权，本轮不执行"
        );
        reply(on_abandoned());
        return;
    }
    // 作业占用 DB 线程时长探针（ADR-0125 决策 2）：阈值与告警口径与既有持锁探针同源。
    let started = Instant::now();
    let guard = match watch.slot().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            // 槽锁中毒（迁移期仍有直锁调用点，闭包 panic 会毒化互斥体）：连接状态
            // 不可信，标记后 fail-loud——不把中毒当作可服务状态吞掉。
            let error = AppError::Db(format!("数据库连接锁已中毒: {poisoned}"));
            trust.mark_untrusted(error.clone());
            reply(Err(error));
            return;
        }
    };
    // 等槽结束的裁决：调用方在等槽期间弃权（时限已到）→ 作业体不执行，只把槽还
    // 回去——「取不到就放弃本轮」的现场语义在门面形态下的落点。
    if gate.try_start() == StartDecision::Cancelled {
        tracing::debug!(
            thread = name,
            command,
            "作业在等槽期间被调用方弃权，本轮不执行"
        );
        reply(on_abandoned());
        return;
    }
    // 换连承接下来到这里（issue #1409）：槽锁在手，代次读数与换连原语的抬高同序。
    if watch.observe_swap() {
        trust.reset_after_swap();
    }
    // 连接已不可信（且本次未换连）：后续作业一律 fail-loud 报同一错误，不静默
    // 恢复服务（ADR-0125 决策 3）。判定基准是标记本身，不是「槽当下能不能锁上」
    // ——标记被清除而槽能锁上时仍拒服务（判据见 `untrusted_connection_fails_loud_*`）。
    // 槽锁已中毒这一路在取锁处先行返回：错误文本由 `PoisonError` 稳定给出，同样
    // fail-loud、同样逐字一致，语义不弱化。
    if let Some(error) = trust.untrusted_error() {
        tracing::error!(thread = name, "数据库连接不可信，作业拒绝执行");
        reply(Err(error));
        return;
    }
    // 上下文承接与阻塞线程池 helper 同一实现（`with_caller_context`）：作业在调用方
    // dispatcher 与 span 内执行，调用点无 span 时重建 `command` span——归因不漂移。
    with_caller_context(&dispatch, &caller_span, command, || {
        // catch_unwind **位于连接槽守卫作用域之内**（ADR-0125 决策 3）：panic 在守卫
        // 释放前被拦下，互斥体不中毒——「panic 后门面仍可服务」的机制前提。
        let outcome = catch_unwind(AssertUnwindSafe(|| run(&guard)));
        // 恢复（回滚 + 连接可信性裁决）也是作业占用 DB 线程的一段，随作业一并计量。
        let report = match outcome {
            Ok(Ok(payload)) => Ok(payload),
            Ok(Err(error)) => Err(error),
            Err(panic) => Err(recover_from_panic(name, &guard, trust, &panic)),
        };
        // 探针在回传前取值：量纲是「作业占用 DB 线程时长」，回传不是作业的一部分
        //（取值晚于回传会与调用方的 await 竞速，告警可能落在 await 之后）。
        // 阈值按作业类取（issue #1765）：备份/恢复等锁内重 IO 作业经壳层启动时登记
        // 的持锁预算自持阈值，其余作业维持 1s 产品阈值。
        probe_lock_hold_within(started.elapsed(), hold_threshold_for(command));
        // 守卫在此释放（panic 已被拦下，不经过守卫析构）。
        drop(guard);
        // 回传与失败日志同在调用方 span 内（归因口径与迁移前同款）。
        reply(report);
    });
}

/// panic 恢复（ADR-0125 决策 3）：回滚未提交事务 → 连接可信则报 panic 错误（该作业
/// 的失败），恢复失败则标记连接不可信并报同一错误（fail-loud，不静默恢复服务）。
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

/// 回滚未提交事务并复核连接回到提交点（判据与失败语义见 `tx_scope::rollback_if_open`）。
///
/// SQLite 无确定性的「回滚失败」注入手段（探针实证见 `db/tests/facade.rs` 注释：
/// 第二连接持共享锁、`query_only`、中途改 journal 模式下 ROLLBACK 均成功），故恢复
/// 失败这条分支的 fail-loud 语义由仅测试构建可见的注入开关驱动——生产构建不含该开关。
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

/// panic 载荷 → 错误（与既有 `run_db` 的 JoinError 归一化同形：`AppError::Io`，
/// 错误码 `io.error` 不变）。
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
    /// 测试注入开关（仅测试构建）：让 DB 线程上下一次 panic 恢复失败。作业体与恢复
    /// 同在该线程执行，故开关随线程局部生效——见 `recover_after_panic`。
    static FORCE_RECOVERY_FAILURE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 测试注入：让 DB 线程上下一次 panic 恢复失败（仅测试构建可见）。
#[cfg(test)]
pub(crate) fn force_next_recovery_failure() {
    FORCE_RECOVERY_FAILURE.with(|flag| flag.set(true));
}

/// 读取并复位测试注入开关（仅测试构建可见）。
#[cfg(test)]
fn force_recovery_failure_pending() -> bool {
    FORCE_RECOVERY_FAILURE.with(|flag| flag.replace(false))
}
