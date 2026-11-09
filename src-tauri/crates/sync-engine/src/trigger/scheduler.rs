//! 同步轮次编排与触发入口（issue #958 拆分自 `trigger.rs`；#863 / ADR-0091
//! 决策 9、ADR-0098；#1339 / ADR-0120 轮次分段取锁与在途互斥）：把「什么时候
//! 同步」的知识收进本域——轮次编排单点、打开应用即同步、桌面运行期低频轮询、
//! 写 op 后去抖合流触发，以及手动入口的码化错误构造。
//!
//! 变更原因单一：改轮询周期、去抖窗口或触发入口分流只动本文件。通道配置与构库
//! 见 [`super::channel`]，会话密钥形态见 [`super::session`]，轮次在途互斥单点见
//! [`super::round_gate`]。
//!
//! - 轮次编排（[`run_round_once`] / [`run_auto_round`]）：跑一轮
//!   [`super::channel::SyncChannel::run_round`] 并在整轮成功后的整体裁决点落
//!   成功时刻；手动入口与自动入口共用同一段轮次协议，只有两个差异——失败
//!   归宿（手动上抛给用户，自动静默等下一轮）与在途轮次的重复触发口径
//!   （ADR-0120 决策 3：手动等待并复用唯一在途轮次的报告，自动放弃本轮）。
//!   两个入口都经轮次在途互斥接线：删除接线即触发侧域单测红（ADR-0087）。
//!   轮次本体分段取锁（ADR-0120 决策 2/4）：连接锁只盖数据库步骤，每段短取；
//!   调度侧任一段拿不到锁即整体放弃本轮（已提交重放与已上传段各自原子留存，
//!   不是回滚前缀；ADR-0120 决策 2/6）。
//! - 自动触发（[`run_auto_round`] / [`sync_on_start_with_host`] /
//!   [`start_sync_scheduler_with_host`] / [`sync_after_write`]）：打开应用即同步 +
//!   运行期低频轮询 + 写 op 后即时入队上传，三者同一段编排，只有触发时机不同。
//!   触发宿主面两形态：平台中立 [`TriggerHost`]（NSH 壳消费，issue #2080）与
//!   tauri 适配（[`sync_on_start`] 等四个旧壳入口，tauri-host 门，issue #2074）
//!   ——编排本体只此一份。写后触发的信号点在**本地 op 产出
//!   单点**（协议 crate `ledger_sync_protocol::op::record_local`，经
//!   [`install_after_write_hook`] 装入响应闭包），去抖合流：连续记账只在最后一次
//!   写后 [WRITE_DEBOUNCE] 跑一轮，避免每次写入都触网（ADR-0091「写 op 后即时
//!   入队上传」的取舍留痕见 ADR-0098）。
//! - 信封模式由**本机会话密钥形态**（[`super::session::SessionEnvelope`]，进程级
//!   单例）判定；换库路径（原位重引导）清空记忆且必须先于换库，重置起点
//!   （忘记口令 / 启动失败）与关闭加密记入明文形态（issue #1395），避免拿旧库
//!   口令去封新库的段。

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::Connection;
// tauri-host 门（issue #2074 / ADR-0145）：AppHandle 触发宿主 API 只被旧壳消费，
// NSH 原生壳进程不得链接 tauri（GTK3/WebKitGTK 与 GTK4 同进程符号互踩）。
// NSH 经平台中立宿主面（[`TriggerHost`]）接线，不走本门。
#[cfg(feature = "tauri-host")]
use tauri::{AppHandle, Manager, Runtime};

use crate::channel::{
    ChannelOptions, ConnSegment, RoundConn, SyncRoundReport, connection_round_key,
};
use crate::envelope::EnvelopeMode;
use ledger_infra::db::LOCK_HOLD_PROBE_THRESHOLD;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
// DbState 与失效信号发射面只被 tauri 适配消费（平台中立宿主面不认识它们）。
#[cfg(feature = "tauri-host")]
use ledger_infra::db::DbState;
use ledger_infra::db::now_iso;
#[cfg(feature = "tauri-host")]
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};
use ledger_infra::error::{AppError, Result};
use ledger_infra::settings::{self, SettingKey};

use super::channel::{SyncChannel, build_channel, configured_channel};
use super::round_gate::{RoundStart, begin_round};
use super::session::SessionEnvelope;

/// 桌面运行期低频轮询的周期（低频：ADR-0091 决策 9「运行期低频轮询」）。
/// 与自动备份同量级（10 分钟，ADR-0016 修订注记），两件事共用同一「低频」品味；
/// 触发时机是可逆工程决策，取舍留痕见 ADR-0098。
const POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// 写后即时同步的去抖窗口：连续记账（批量录入、导入）只在最后一次写后
/// [WRITE_DEBOUNCE] 触发一轮，避免「每写一笔就触网一次」（ADR-0098 取舍留痕）。
const WRITE_DEBOUNCE: Duration = Duration::from_secs(5);

/// 触发时机的显式参数（调度线程的轮询周期与写后去抖窗口）：生产走默认值
/// （即上两个常量），触发接线测试注入「短去抖 + 超长轮询」——写后触发断言
/// 只等去抖窗，且接线断掉（写信号删除）时不得被低频轮询救活（issue #959）。
/// `ChannelOptions` 同款显式参数接缝（KDF 迭代次数与段容量的先例），不是
/// 测试后门：参数是调度线程的正常输入，任何调用方都可给定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggerTimings {
    /// 低频轮询周期（兜底「对端有新数据」与写后失败重试）。
    pub poll_interval: Duration,
    /// 写后去抖窗口（连续记账合流成一轮的静默判定长度）。
    pub write_debounce: Duration,
}

impl Default for TriggerTimings {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            write_debounce: WRITE_DEBOUNCE,
        }
    }
}

/// 在途轮次的重复触发口径（ADR-0120 决策 3）：手动与自动入口的分野。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InFlightPolicy {
    /// 手动入口：等待并交出唯一在途轮次的报告（回显形态在 #1339 定夺留痕：
    /// 取「等待并复用」，与 ADR-0095 前端口径同款且不改 `sync_now` wire 契约）。
    WaitAndReuse,
    /// 自动入口：放弃本轮（静默，与「拿不到连接锁就跳过本轮」同口径）。
    Skip,
}

/// 在途互斥的消费结果（[`run_round_gated`] 的返回形态）。
enum GatedRound {
    /// 本轮由本执行体跑完（整体裁决点已落成功时刻）。
    Ran(SyncRoundReport),
    /// 复用唯一在途轮次：等待其结果并原样交出（成功时刻由在途轮次落库）。
    Joined(SyncRoundReport),
    /// 自动入口撞上在途轮次：放弃本轮。
    Skipped,
}

/// 执行一轮同步并更新「上次成功同步时刻」（手动入口的编排单点，ADR-0120 决策
/// 3/4）：发布自己流新 op + 拉取他人流增量并经引擎幂等重放；成功即在整体裁决
/// 点（落库段）落成功时刻（失败不更新，也不产生部分静默状态——已上传段与已
/// 应用重放各自原子，ADR-0120 决策 6）。连接只经 [`RoundConn`] 接缝按数据库段
/// 短取，网络段不消费连接；同端顺序性由轮次在途互斥承接——在途时重复触发不
/// 启动第二轮，等待并交出同一轮次报告。
pub fn run_round_once<S: RoundConn>(
    locks: &S,
    channel: &SyncChannel,
    mode: &EnvelopeMode<'_>,
) -> Result<SyncRoundReport> {
    match run_round_gated(locks, channel, mode, InFlightPolicy::WaitAndReuse) {
        Ok(GatedRound::Ran(report)) | Ok(GatedRound::Joined(report)) => Ok(report),
        Ok(GatedRound::Skipped) => Err(AppError::Invalid(
            "在途等待未获轮次报告（程序缺陷）".to_string(),
        )),
        Err(error) => Err(error),
    }
}

/// 执行一次自动轮次（打开应用即同步与低频轮询共用）：通道未配置、在途互斥
/// 撞车（已有唯一在途轮次，ADR-0120 决策 3）或任一段拿不到连接锁（整体放弃
/// 本轮，既有「拿不到锁就跳过本轮」口径）均回 `None`；其余按给定会话形态
/// 跑一轮。
///
/// 调用方是自动入口（[`sync_on_start_with_host`] / [`start_sync_scheduler_with_host`]）：失败由调用方
/// 吞掉记日志，不影响本地记账；手动「立即同步」不走本函数（走 [`run_round_once`]，
/// 失败上抛给用户）。
pub fn run_auto_round<S: RoundConn>(
    locks: &S,
    session: &SessionEnvelope,
) -> Result<Option<SyncRoundReport>> {
    match run_auto_round_inner(locks, session) {
        // 拿不到连接锁：放弃本轮（静默；已上传段与已提交重放各自原子留存，
        // 下一轮幂等续作，ADR-0120 决策 2/6）。
        Err(error) if is_round_lock_give_up(&error) => Ok(None),
        other => other,
    }
}

/// 自动轮次本体：配置读取（读段，短取锁）→ 构库 → 在途互斥下的轮次。
fn run_auto_round_inner<S: RoundConn>(
    locks: &S,
    session: &SessionEnvelope,
) -> Result<Option<SyncRoundReport>> {
    let Some(config) = locks.with_connection(ConnSegment::Read, configured_channel)? else {
        return Ok(None);
    };
    let channel = build_channel(&config)?;
    Ok(
        match run_round_gated(locks, &channel, &session.mode(), InFlightPolicy::Skip)? {
            GatedRound::Ran(report) | GatedRound::Joined(report) => Some(report),
            GatedRound::Skipped => None,
        },
    )
}

/// 在途互斥接线（全部轮次入口的必经点，ADR-0120 决策 3）：登记在途后跑轮次
/// 并在整体裁决点落成功时刻；撞上在途轮次按入口口径等待或放弃。
///
/// **负向判据接线点（ADR-0087）**：删除本函数的 [`begin_round`] 调用，
/// 触发侧域单测即红（自动入口不再跳过、手动入口不再复用在途报告）。
fn run_round_gated<S: RoundConn>(
    locks: &S,
    channel: &SyncChannel,
    mode: &EnvelopeMode<'_>,
    policy: InFlightPolicy,
) -> Result<GatedRound> {
    match begin_round(locks.round_key()) {
        RoundStart::Began(handle) => {
            let outcome = run_round_and_bookkeep(locks, channel, mode);
            handle.complete(outcome.clone());
            match outcome {
                Ok(report) => Ok(GatedRound::Ran(report)),
                Err(error) => Err(error),
            }
        }
        RoundStart::InFlight(slot) => match policy {
            InFlightPolicy::WaitAndReuse => Ok(GatedRound::Joined(slot.wait()?)),
            InFlightPolicy::Skip => Ok(GatedRound::Skipped),
        },
    }
}

/// 轮次本体 + 落库段：整轮成功后的整体裁决点才更新「上次成功同步时刻」
/// （ADR-0120 决策 6：中途失败 / 放弃不更新）。
fn run_round_and_bookkeep<S: RoundConn>(
    locks: &S,
    channel: &SyncChannel,
    mode: &EnvelopeMode<'_>,
) -> Result<SyncRoundReport> {
    let report = channel.run_round(locks, mode, &ChannelOptions::default())?;
    locks.with_connection(ConnSegment::Bookkeep, |conn| {
        settings::set(conn, SettingKey::SyncLastSyncAt, &now_iso())
    })?;
    Ok(report)
}

/// 连接锁在时限内不可得的码化错误：调度侧放弃本轮的内部信号——自动入口静默
/// 消化（[`is_round_lock_give_up`] 判定），从不上抛给用户。
fn round_lock_give_up_error() -> AppError {
    AppError::coded(
        ROUND_LOCK_GIVE_UP,
        "连接锁在时限内不可得，放弃本轮同步（下一轮重试）",
    )
}

/// [`round_lock_give_up_error`] 的稳定码（判定单点）。
const ROUND_LOCK_GIVE_UP: &str = "sync-engine.round-lock-give-up";

/// 放弃本轮的判定单点：恰好命中放弃码，不误伤轮次的真实失败（真实失败照常
/// 上抛记日志）。
fn is_round_lock_give_up(error: &AppError) -> bool {
    error.is_code(ROUND_LOCK_GIVE_UP)
}

/// 调度侧轮次连接源（ADR-0120 决策 4：调度侧与手动入口同形分段取锁）：每段
/// 经连接锁等待单点（`ledger_backup::lock_conn_with_timeout`）短暂取一次连接，
/// 任一段拿不到锁即以 [`ROUND_LOCK_GIVE_UP`] 放弃本轮——放弃只终止后续段，
/// 已提交的重放与已上传的段各自原子留存。持锁时长照守（探针超阈值不静默，
/// #1276 守门③）。
pub(crate) struct AutoRoundConn {
    conn: Arc<Mutex<Connection>>,
    /// 每段取连接锁的等待上限。产品路径取 [`ledger_backup::LOCK_TIMEOUT`]（经
    /// [`AutoRoundConn::new`]），与备份调度同值同口径；测试可经
    /// [`AutoRoundConn::with_lock_timeout`] 注入短超时——「拿不到锁即放弃本轮」
    /// 是瞬时可判定的语义，按产品默认值实等只贡献墙钟（spec #1086 / issue #1514）。
    lock_timeout: Duration,
    /// 持锁时长探针阈值。产品路径取 [`ledger_infra::db::LOCK_HOLD_PROBE_THRESHOLD`]；
    /// 测试可经 [`AutoRoundConn::with_probe_threshold`] 注入短阈值——「超阈值即记
    /// warn」同样是瞬时可判定语义，实等 1.1s 只为越过产品阈值（spec #1086 / #1514）。
    probe_threshold: Duration,
}

impl AutoRoundConn {
    /// 以共享连接句柄构造（与壳层写入口、备份调度同一句柄源：`DbState.conn`）。
    pub(crate) fn new(conn: &Arc<Mutex<Connection>>) -> Self {
        Self {
            conn: Arc::clone(conn),
            lock_timeout: ledger_backup::LOCK_TIMEOUT,
            probe_threshold: LOCK_HOLD_PROBE_THRESHOLD,
        }
    }

    /// 覆盖每段取锁的等待上限（仅供测试注入短超时；产品路径不得调用）。
    #[cfg(test)]
    pub(crate) fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// 覆盖持锁时长探针阈值（仅供测试注入短阈值；产品路径不得调用）。
    #[cfg(test)]
    pub(crate) fn with_probe_threshold(mut self, threshold: Duration) -> Self {
        self.probe_threshold = threshold;
        self
    }
}

impl RoundConn for AutoRoundConn {
    fn with_connection<R, F>(&self, _segment: ConnSegment, use_connection: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R>,
    {
        let hold_started = Instant::now();
        let Some(guard) = ledger_backup::lock_conn_within(&self.conn, self.lock_timeout) else {
            return Err(round_lock_give_up_error());
        };
        let result = use_connection(&guard);
        // 持锁时长探针（#1276 守门③）：轮次的每一段各自接哨，整段形态的长持锁
        // （如误把网络等待写回段内）在此现形。
        ledger_infra::db::probe_lock_hold_within(hold_started.elapsed(), self.probe_threshold);
        result
    }

    fn round_key(&self) -> u64 {
        connection_round_key(&self.conn)
    }
}

/// 平台中立触发宿主（issue #2080，注册点反转 ADR-0112 决策 5 的域侧形态）：
/// 触发编排的非 tauri 宿主面——把 AppHandle 在触发路径上的实际用途收拢成
/// 三个显式输入（连接与两扇门、轮次完成回调），编排本体只此一份（tauri 面
/// 与 NSH 面都是它的消费方，不改「什么时候同步」的域内单点语义）。
///
/// - `conn`：与壳层写入口、备份调度同一句柄源（`DbState.conn`）。
/// - `gate` / `boot_gate`：调度循环的门检输入（锁定/启动失败期间空转）；宿主
///   按自己的状态机供给——旧壳取 `AppHandle` 管理态，NSH 立桩只开明文库且
///   只在引导成功分支接线，以恒开门态注入（编排语义不变，门检仍在域内一处）。
/// - `on_round_applied`：自动轮次完成回调，`applied > 0` 即真（本轮实际应用
///   外来 op）。旧壳实现投递 `ledger:changed` 失效事件（`emit_for`，空信号集
///   零投递）；NSH 实现广播进程内失效信号（经通道回引擎线程）。在调度线程
///   上调用，实现方自持线程契约（不阻塞轮次）。
pub struct TriggerHost {
    /// 轮次分段取锁的连接句柄源。
    pub conn: Arc<Mutex<Connection>>,
    /// 加密门（锁定期间空转）。
    pub gate: EncryptionGate,
    /// 启动失败门（失败占位期间空转）。
    pub boot_gate: BootFailureGate,
    /// 轮次完成回调：`applied > 0` 即本轮应用了外来 op（失效信号的证据位）。
    pub on_round_applied: Arc<dyn Fn(bool) + Send + Sync>,
}

/// 打开应用即同步（业务可用起点调用，ADR-0091 决策 9）：一次性后台轮次，
/// 失败静默记日志——打开应用不该被网络/凭据问题打断。平台中立宿主面
///（[`TriggerHost`]，issue #2080）：NSH 壳经它接线，旧壳经 [`sync_on_start`
/// （tauri 适配）消费同一编排。
///
/// 本函数**平台无关**（移动端也跑）：打开即同步是 Android 的兜底语义。
pub fn sync_on_start_with_host(host: &Arc<TriggerHost>) {
    let host = Arc::clone(host);
    std::thread::spawn(move || {
        run_auto_round_with_host(&host, &SessionEnvelope::current());
    });
}

/// 业务可用起点的**触发编排单一入口**（启动就绪、解锁成功、启动失败重置三条
/// 路径共用，ADR-0098 决策 4）：分流「拉不拉低频轮询线程」只在本函数一处。
/// 平台中立宿主面（[`TriggerHost`]，issue #2080）；旧壳经 [`start_triggers`
/// （tauri 适配）消费同一编排。
///
/// - **打开即同步**：全平台都跑（Android 后台不承诺同步，打开即同步是兜底
///   语义，工单 10 零分叉接入）。
/// - **低频轮询 + 写后触发**：仅桌面。移动端系统会回收后台进程，轮询线程不保证
///   存活，承诺「后台同步」是空头支票（ADR-0091 决策 9）；用 `#[cfg(desktop)]`
///   在编译期剔除，与 ADR-0074 决策 6 的既有分平台先例同款。
///
/// 两个调度各持单次拉起守卫，原位重引导/重复调用幂等。分平台门只此一处
///（旧壳源码扫描守门 sync_trigger_guard 钉住）；`timings` 是调度线程的正常
/// 输入（显式参数接缝，issue #959：触发接线测试注入短时机）。
pub fn start_triggers_with_host(host: &Arc<TriggerHost>, timings: TriggerTimings) {
    #[cfg(desktop)]
    start_sync_scheduler_with_host(host, timings);
    sync_on_start_with_host(host);
}

/// 写后触发去抖合流的**决策本体**（ADR-0091 决策 9「写 op 后即时入队上传」）：
/// 把去抖窗口内的连续写信号一次吸干，静默 `window` 后返回。
///
/// 与线程时序解耦，便于直接断言「连续记账合流成一轮」；调度线程只做
/// `recv_timeout` 循环 + 调用本函数（`window` 即静默判定长度）。
///
/// **契约**：调用方已消费掉触发本次收尾的那个写信号（外层 `recv_timeout` 的
/// `Ok(())` 分支）——单笔记账（窗口内无后续写）也会在 `window` 静默后返回并跑
/// 一轮，不会漏掉最后一批 op。窗口内继续收到的写信号被吸干、合流进同一轮
/// （不残留信号触发第二轮）。
pub(crate) fn drain_write_signals(rx: &std::sync::mpsc::Receiver<()>, window: Duration) {
    loop {
        match rx.recv_timeout(window) {
            Ok(()) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// 写后即时同步的信号通道（进程级单例）：本地 op 产出单点（协议 crate
/// `record_local`，经 [`install_after_write_hook`] 装入的钩子）触发
/// [`sync_after_write`] 投递一次「有新 op 待发布」，调度线程据此起一轮。发送端
/// 未装（调度未拉起 / 单测环境）时投递是零动作——写路径对同步域完全无感。
static WRITE_SIGNAL: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

/// 写后触发的**登记点反转**（#1089）：本地 op 产出单点在协议 crate（共享底座，
/// 不认识同步调度），本域在启动时把「去抖合流跑一轮」的响应闭包装进协议面的
/// 钩子槽（`ledger_sync_protocol::op::install_after_write_hook`）。幂等：重复
/// 安装零动作（先装者优先）；生产启动（壳层 `setup`）、测试建库工厂与 BDD
/// world 各装一次，闭包同体。
pub fn install_after_write_hook() {
    let installed = ledger_sync_protocol::op::install_after_write_hook(sync_after_write);
    if !installed {
        tracing::debug!("写后钩子已安装，忽略重复登记（先装者优先）");
    }
}

/// 写 op 后即时入队上传的信号侧（ADR-0091 决策 9）：本地产出 op 后调用一次。
///
/// 触发点是**本地 op 产出单点**（协议 crate [`ledger_sync_protocol::op::record_local`]，
/// 经 [`install_after_write_hook`] 装入本函数为响应）：本仓全部本地产出（IPC /
/// HTTP / 批量导入 / 定时追补）都经它收敛，且外来 op 重放走协议面 `insert_row`
/// 不产出本地 op、天然不误触发。放在壳层 `write_entry` 会漏掉定时追补（直接
/// 进行为编排）并在 `sync_now` 自身空触发；放在连接层统一写入口提交点则既有
/// 分层问题又不区分产出。**非阻塞**：无界通道投递即返回，写路径不等网络；
/// 发送端缺席（未拉起调度、单测）静默忽略。去抖合流在调度线程侧完成。
pub fn sync_after_write() {
    notify_write_signal(&WRITE_SIGNAL);
}

/// 写信号投递（信号侧单点）：通道未装时零动作。与 [`drain_write_signals`]
/// 对称——投递与吸干两侧都以通道为显式参数，可脱离线程时序单独断言
/// （不依赖进程级单例，无需测试后门）。
pub(crate) fn notify_write_signal(slot: &std::sync::OnceLock<std::sync::mpsc::Sender<()>>) {
    if let Some(tx) = slot.get() {
        let _ = tx.send(());
    }
}

/// 写后触发与低频轮询的统一调度线程（幂等：单次拉起守卫，与自动备份调度同型）。
///
/// 生产入口：时机参数走默认常量（[`TriggerTimings::default`]）；触发接线测试
/// 注入更短窗口经编排入口的显式参数（[`start_triggers_with_host`]，issue #959）。
///
/// 单一线程承载两条触发路径，避免并发触网：
/// - **写后即时**（[`sync_after_write`] 的信号）：收到信号后先去抖 [WRITE_DEBOUNCE]
///   窗口（窗口内再有写入则继续等待），再跑一轮——连续记账合流成一轮；
/// - **低频轮询**：`recv_timeout` 到期即跑一轮（兜底「对端有新数据」与写后失败重试）。
///
/// 每轮门检锁定/启动失败期间空转（占位连接不是业务库）；轮次分段取锁（ADR-0120
/// 决策 4）：拿不到连接锁的段放弃本轮（静默）；与在途轮次撞车同样放弃（在途
/// 互斥，ADR-0120 决策 3）；失败静默记录，等下一轮重试。平台中立宿主面
///（[`TriggerHost`]，issue #2080）。
pub fn start_sync_scheduler_with_host(host: &Arc<TriggerHost>, timings: TriggerTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    if WRITE_SIGNAL.set(tx).is_err() {
        // 已有发送端（理论不可能：单次拉起守卫）：退化为纯轮询。
        tracing::warn!("同步调度已存在写信号通道，写后触发退化为低频轮询");
    }
    let host = Arc::clone(host);
    std::thread::spawn(move || {
        loop {
            match rx.recv_timeout(timings.poll_interval) {
                // 写后触发：去抖合流——窗口内继续有写则顺延，直到静默
                // [WRITE_DEBOUNCE] 才跑一轮（连续记账合流成一轮，最后一批 op 不丢）。
                Ok(()) => {
                    // 去抖合流：本信号已计一次写，把窗口内后续写信号一次吸干
                    // （连续记账合流成一轮），静默 [WRITE_DEBOUNCE] 后跑一轮。
                    drain_write_signals(&rx, timings.write_debounce);
                }
                // 低频轮询到期（或写信号通道断裂后的兜底）。
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if host.gate.is_locked() || host.boot_gate.is_failed() {
                continue;
            }
            run_auto_round_with_host(&host, &SessionEnvelope::current());
        }
    });
}

/// 自动轮次 + 轮次完成回调：本轮实际应用外来 op（`applied > 0`）即交由宿主
/// 投递失效信号（旧壳 `emit_for` → `ledger:changed` 事件；NSH 进程内失效信号
/// 广播——与手动同步同一证据位 `SyncRound`/`LedgerApplied`，见 infra
/// `signals_for` 的映射行）——同步轮次落地的数据同样要让各视图重拉，否则
///「同步了但界面不动」。
///
/// 失败静默（ADR-0091：同步失败不阻塞本地记账），记日志等下一轮。轮次分段
/// 取锁（ADR-0120 决策 4）：数据库步骤经 [`AutoRoundConn`] 每段短取连接锁，
/// 网络段与本地写、与其它命令并行。
fn run_auto_round_with_host(host: &TriggerHost, session: &SessionEnvelope) {
    let locks = AutoRoundConn::new(&host.conn);
    match run_auto_round(&locks, session) {
        Ok(Some(report)) => {
            tracing::debug!(
                applied = report.applied,
                parked = report.parked,
                "同步轮次完成"
            );
            (host.on_round_applied)(report.applied > 0);
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "同步轮次失败（静默，等下一轮）"),
    }
}

/// tauri 宿主适配（tauri-host 门，issue #2074）：从 AppHandle 管理态装配
/// [`TriggerHost`]——连接与两扇门取 state，轮次完成回调投递 `ledger:changed`
/// 失效事件（`emit_for` 按信号映射判定，`applied == false` 时空信号集零投递，
/// 与 NSH 侧 `if applied` 等价）。
#[cfg(feature = "tauri-host")]
fn host_from_app<R: Runtime>(app: &AppHandle<R>) -> Arc<TriggerHost> {
    Arc::new(TriggerHost {
        conn: Arc::clone(&app.state::<DbState>().conn),
        gate: EncryptionGate::clone(&app.state::<EncryptionGate>()),
        boot_gate: BootFailureGate::clone(&app.state::<BootFailureGate>()),
        on_round_applied: {
            let handle = app.clone();
            Arc::new(move |applied: bool| {
                emit_for(
                    &handle,
                    WriteOp::SyncRound,
                    WriteEvidence::LedgerApplied(applied),
                );
            })
        },
    })
}

/// 打开应用即同步的 tauri 适配（签名与行为不变，issue #2074 门化面）：装配
/// [`TriggerHost`] 后消费平台中立编排（[`sync_on_start_with_host`]）。
#[cfg(feature = "tauri-host")]
pub fn sync_on_start<R: Runtime>(app: &AppHandle<R>) {
    sync_on_start_with_host(&host_from_app(app));
}

/// 触发编排单一入口的 tauri 适配（签名与行为不变，issue #2074 门化面）：装配
/// [`TriggerHost`] 后消费平台中立编排（[`start_triggers_with_host`]，分平台门
/// 收在域内一处）。
#[cfg(feature = "tauri-host")]
pub fn start_triggers<R: Runtime>(app: &AppHandle<R>) {
    start_triggers_with_host(&host_from_app(app), TriggerTimings::default());
}

/// 低频轮询调度线程拉起的 tauri 适配（签名与行为不变，issue #2074 门化面）：
/// 生产时机走默认常量，经编排入口 [`start_triggers`]（或本函数）进入。
#[cfg(feature = "tauri-host")]
pub fn start_sync_scheduler<R: Runtime>(app: &AppHandle<R>) {
    start_sync_scheduler_with_host(&host_from_app(app), TriggerTimings::default());
}

/// 调度线程拉起（显式时机参数版）的 tauri 适配（签名与行为不变，issue #2074
/// 门化面）：轮询周期与写后去抖窗口由调用方给定——生产传
/// [`TriggerTimings::default`]，触发接线测试注入「短去抖 + 超长轮询」（写后
/// 触发断言只等去抖窗，不被低频轮询救活，issue #959）。其余语义同
/// [`start_sync_scheduler`]。
#[cfg(feature = "tauri-host")]
pub fn start_sync_scheduler_with<R: Runtime>(app: &AppHandle<R>, timings: TriggerTimings) {
    start_sync_scheduler_with_host(&host_from_app(app), timings);
}

/// 通道未配置的码化错误（手动「立即同步」入口；自动入口静默跳过）。
pub fn not_configured_error() -> AppError {
    AppError::coded(
        "sync-channel.not-configured",
        "同步通道尚未配置，请先在设置中填写同步通道信息",
    )
}

/// 账本注册表不可用的码化错误（手动入口的守卫）。
pub fn book_unavailable_error() -> AppError {
    AppError::coded(
        "sync-channel.book-unavailable",
        "账本注册表不可用，无法确定同步范围",
    )
}
