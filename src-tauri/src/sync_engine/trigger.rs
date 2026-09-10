//! 同步触发编排（issue #863 / ADR-0091 决策 9、ADR-0098）：把「什么时候同步」
//! 的知识从壳层收进本域——打开应用即同步、桌面运行期低频轮询、手动「立即同步」
//! 三条入口共用同一段轮次编排与同一份通道配置读取。
//!
//! 职责边界：
//! - 通道配置（[`SyncChannelConfig`]）与其构库校验（[`build_channel`]）在本域
//!   单点——壳层不再各自解析凭据与布局（#862 曾把该校验放在壳层）；
//! - 轮次编排（[`run_round_once`]）：一轮 [`super::channel::run_round_with`] +
//!   成功时刻落库，手动入口与自动入口共用；手动入口的失败上抛给用户，自动入口
//!   的失败静默等下一轮（ADR-0091：「同步失败不阻塞本地记账」）；
//! - 自动触发（[`run_auto_round`] / [`sync_on_start`] / [`start_sync_scheduler`] /
//!   [`sync_after_write`]）：打开应用即同步 + 运行期低频轮询 + 写 op 后即时入队
//!   上传，三者同一段编排，只有触发时机不同。写后触发的信号点在**本地 op 产出
//!   单点** [`super::ops::record_local`]（域内，见该函数文档），去抖合流：连续记账
//!   只在最后一次写后 [WRITE_DEBOUNCE] 跑一轮，避免每次写入都触网（ADR-0091
//!   「写 op 后即时入队上传」的取舍留痕见 ADR-0098）；
//! - 信封模式由**本机会话密钥形态**（[`SessionEnvelope`]，进程级单例）判定：
//!   解锁密文库或一次成功的手动同步后记入，有则封包、无则明文直通——自动轮询
//!   不读钥匙串（钥匙串读取在发布构建下带生物认证门，后台轮询不得弹交互，
//!   ADR-0098）。换库路径（原位重引导、忘记口令重置）清空记忆、关闭加密记入
//!   明文形态，避免拿旧库口令去封新库的段。
//!
//! 触发时机是**可逆工程决策**（ADR-0091「后果」段明言不入 ADR），轮询周期与
//! 「自动轮询不做新端引导」两条留痕见 ADR-0098。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::db::boot::BootFailureGate;
use crate::db::encryption::EncryptionGate;
use crate::db::{DbState, now_iso};
use crate::error::{AppError, Result};
use crate::settings::{self, SettingKey};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

use super::channel::{ChannelLayout, ChannelOptions, SyncRoundReport, run_round_with};
use super::envelope::EnvelopeMode;
use super::transport::webdav::{WebDavConfig, WebDavTransport};

/// 通道空间默认值（同步世界身份的 v1 共识形态，见 [`SyncChannelConfig::space_id`]）。
pub(crate) const DEFAULT_SPACE_ID: &str = "default";

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

/// 通道配置持久化形态（`app_settings` 的 `sync.channel.config`，JSON 对象）。
///
/// 本机设备配置（不同步，同步边界见多端同步域 SyncBoundary）：凭据不随信封走，
/// 新端各自配置。写入经设置域单点（壳层 `set_sync_channel_config`），读取经
/// [`configured_channel`] 单点。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SyncChannelConfig {
    /// 同步根目录 URL。
    pub base_url: String,
    /// WebDAV 账号。
    pub username: String,
    /// WebDAV 密码 / 应用密码。
    pub password: String,
    /// 同步空间（通道世界身份，`book-<space>` 目录）：**跨端共识值**——两端
    /// 配置同一同步空间即对齐同一世界。账本登记的标识按 ADR-0089 是本机
    /// 事实（目录哈希 / 本机生成 UUID），跨设备不成立，故世界身份由用户在
    /// 通道配置里显式约定（v1 默认 `default`）；多账本各自独立同步 = 各配
    /// 一个空间。反序化缺省回默认（旧配置升级）。
    #[serde(default = "default_space_id")]
    pub space_id: String,
}

/// [`SyncChannelConfig::space_id`] 的 serde 缺省值单点。
fn default_space_id() -> String {
    DEFAULT_SPACE_ID.to_string()
}

/// 通道句柄（传输 + 布局）：由配置构库一次、轮次复用。
#[derive(Debug)]
pub struct SyncChannel {
    transport: WebDavTransport,
    layout: ChannelLayout,
}

impl SyncChannel {
    /// 传输后端（轮次编排消费；壳层不直接触达）。
    pub fn transport(&self) -> &WebDavTransport {
        &self.transport
    }

    /// 通道布局（同上）。
    pub fn layout(&self) -> &ChannelLayout {
        &self.layout
    }
}

/// 读取已配置的通道（未配置回 `None`；缺 key / 缺表按设置域读口径回默认）。
pub fn configured_channel(conn: &Connection) -> Result<Option<SyncChannelConfig>> {
    settings::get(conn, SettingKey::SyncChannelConfig, None)
}

/// 从配置构库（连接参数定型 + 布局构造，不发网络请求；错误码复用域的
/// `sync-channel.*`）。保存配置与轮次入口共用本单点，校验序列不重复。
pub fn build_channel(config: &SyncChannelConfig) -> Result<SyncChannel> {
    let transport = WebDavTransport::new(WebDavConfig {
        base_url: config.base_url.clone(),
        username: config.username.clone(),
        password: config.password.clone(),
    })?;
    let layout = ChannelLayout::new(&config.space_id)?;
    Ok(SyncChannel { transport, layout })
}

/// 执行一轮同步并更新「上次成功同步时刻」（手动与自动入口共用的编排单点）：
/// 发布自己流新 op + 拉取他人流增量并经引擎幂等重放；成功即落成功时刻
/// （失败不更新，也不产生部分静默状态——已上传段与已应用重放各自原子）。
pub fn run_round_once(
    conn: &Connection,
    channel: &SyncChannel,
    mode: &EnvelopeMode<'_>,
) -> Result<SyncRoundReport> {
    let report = run_round_with(
        conn,
        channel.transport(),
        channel.layout(),
        mode,
        &ChannelOptions::default(),
    )?;
    settings::set(conn, SettingKey::SyncLastSyncAt, &now_iso())?;
    Ok(report)
}

/// 本会话密钥记忆（ADR-0098）：解锁密文库或一次成功的手动同步后记入，
/// 同步轮次据此判定信封模式。进程级单例，与解锁态同生命周期。
///
/// 归本域而非基础设施：它是**同步触发**的工程决策（自动轮询不得弹生物认证），
/// 除同步轮次外无消费者。口令本体的钥匙串缓存仍归备份域基础设施
/// （`db::passphrase_cache`），本单例只持「本会话已知的形态」。
static SESSION_ENVELOPE: std::sync::Mutex<Option<SessionEnvelope>> = std::sync::Mutex::new(None);

/// 本机会话的密钥形态：解锁密文库或一次成功的手动同步后记入，同步轮次
/// 据此判定信封模式。
///
/// 自动轮询**不读钥匙串**：钥匙串读取在发布构建下先过 LocalAuthentication 门
/// （弹 Touch ID，ADR-0075 决策 3 / issue #866），后台轮询不得弹交互；本会话
/// 没有密钥知识时按明文形态跑（明文库的正常形态），密文库未解锁时业务 IPC
/// 本就不可达（门禁拦截）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnvelope {
    /// 明文库会话：同步明文直通（界面显著提示，ADR-0091 决策 8）。
    Plaintext,
    /// 密文库会话：会话口令在场，同步以之封包。
    Encrypted(String),
}

impl SessionEnvelope {
    /// 记入本会话形态（解锁成功 / 一次成功的手动同步 / 用户设置记住口令）。
    pub fn remember(session: SessionEnvelope) {
        *SESSION_ENVELOPE.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
    }

    /// 清空本会话记忆（引导换库、忘记口令重置、关闭加密等改变库身份的路径）：
    /// 新库形态未知，等下一次解锁/手动同步重新记入——避免拿旧库口令去封新库的段
    /// （密文/明文错配会让对端无法开封）。
    pub fn forget() {
        *SESSION_ENVELOPE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// 读取当前会话形态（未记入回 [`SessionEnvelope::Plaintext`]：明文库形态）。
    pub fn current() -> Self {
        SESSION_ENVELOPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or(SessionEnvelope::Plaintext)
    }

    /// 对应的信封模式（借出形态，轮次消费）。
    pub fn mode(&self) -> EnvelopeMode<'_> {
        match self {
            Self::Plaintext => EnvelopeMode::Plaintext,
            Self::Encrypted(passphrase) => EnvelopeMode::Encrypted { passphrase },
        }
    }
}

/// 执行一次自动轮次（打开应用即同步与低频轮询共用）：通道未配置回 `None`，
/// 其余按给定会话形态跑一轮。
///
/// 调用方是自动入口（[`sync_on_start`] / [`start_sync_scheduler`]）：失败由调用方
/// 吞掉记日志，不影响本地记账；手动「立即同步」不走本函数（走 [`run_round_once`]，
/// 失败上抛给用户）。
pub fn run_auto_round(
    conn: &Connection,
    session: &SessionEnvelope,
) -> Result<Option<SyncRoundReport>> {
    let Some(config) = configured_channel(conn)? else {
        return Ok(None);
    };
    let channel = build_channel(&config)?;
    Ok(Some(run_round_once(conn, &channel, &session.mode())?))
}

/// 打开应用即同步（业务可用起点调用，ADR-0091 决策 9）：一次性后台轮次，
/// 失败静默记日志——打开应用不该被网络/凭据问题打断。
///
/// 与 [`start_sync_scheduler`] 同一段编排，只有触发时机不同。
///
/// 本函数**平台无关**（移动端也跑）：打开即同步是 Android 的兜底语义。签名对
/// 运行时泛型（壳层命令同款）：业务可用起点的现场（真应用 / 测试 mock 应用）
/// 都能调用（issue #959 接线测试）。
pub fn sync_on_start<R: Runtime>(app: &AppHandle<R>) {
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let handle = app.clone();
    std::thread::spawn(move || {
        let Some(guard) = crate::backup::lock_conn_with_timeout(&conn) else {
            return;
        };
        run_auto_round_with_emit(&handle, &guard, &SessionEnvelope::current());
    });
}

/// 业务可用起点的**触发编排单一入口**（启动就绪、解锁成功、启动失败重置三条
/// 路径共用，ADR-0098 决策 4）：分流「拉不拉低频轮询线程」只在本函数一处。
///
/// - **打开即同步**：全平台都跑（Android 后台不承诺同步，打开即同步是兜底
///   语义，工单 10 零分叉接入）。
/// - **低频轮询 + 写后触发**：仅桌面。移动端系统会回收后台进程，轮询线程不保证
///   存活，承诺「后台同步」是空头支票（ADR-0091 决策 9）；用 `#[cfg(desktop)]`
///   在编译期剔除，与 ADR-0074 决策 6 的既有分平台先例同款。
///
/// 两个调度各持单次拉起守卫，原位重引导/重复调用幂等。
pub fn start_triggers<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(desktop)]
    start_sync_scheduler(app);
    sync_on_start(app);
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
fn drain_write_signals(rx: &std::sync::mpsc::Receiver<()>, window: Duration) {
    loop {
        match rx.recv_timeout(window) {
            Ok(()) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// 写后即时同步的信号通道（进程级单例）：本地 op 产出单点
/// [`super::ops::record_local`] 经 [`sync_after_write`] 投递一次「有新 op 待发布」，
/// 调度线程据此起一轮。发送端未装（调度未拉起 / 单测环境）时投递是零动作——
/// 写路径对同步域完全无感。
static WRITE_SIGNAL: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

/// 写 op 后即时入队上传的信号侧（ADR-0091 决策 9）：本地产出 op 后调用一次。
///
/// 触发点是**本地 op 产出单点** [`super::ops::record_local`]：本仓全部本地产出
/// （IPC / HTTP / 批量导入 / 定时追补）都经它收敛，且外来 op 重放走
/// [`super::ops::insert_row`] 不产出本地 op，天然不误触发。放在壳层
/// `write_entry` 会漏掉定时追补（直接进行为编排）并在 `sync_now` 自身空触发；
/// 放在连接层 `db::write` 提交点则既有分层问题又不区分产出。**非阻塞**：无界
/// 通道投递即返回，写路径不等网络；发送端缺席（未拉起调度、单测）静默忽略。
/// 去抖合流在调度线程侧完成。
pub fn sync_after_write() {
    notify_write_signal(&WRITE_SIGNAL);
}

/// 写信号投递（信号侧单点）：通道未装时零动作。与 [`drain_write_signals`]
/// 对称——投递与吸干两侧都以通道为显式参数，可脱离线程时序单独断言
/// （不依赖进程级单例，无需测试后门）。
fn notify_write_signal(slot: &std::sync::OnceLock<std::sync::mpsc::Sender<()>>) {
    if let Some(tx) = slot.get() {
        let _ = tx.send(());
    }
}

/// 写后触发与低频轮询的统一调度线程（幂等：单次拉起守卫，与自动备份调度同型）。
///
/// 生产入口：时机参数走默认常量（[`TriggerTimings::default`]）；触发接线测试
/// 注入更短窗口经 [`start_sync_scheduler_with`]（显式参数接缝，issue #959）。
///
/// 单一线程承载两条触发路径，避免并发触网：
/// - **写后即时**（[`sync_after_write`] 的信号）：收到信号后先去抖 [WRITE_DEBOUNCE]
///   窗口（窗口内再有写入则继续等待），再跑一轮——连续记账合流成一轮；
/// - **低频轮询**：`recv_timeout` 到期即跑一轮（兜底「对端有新数据」与写后失败重试）。
///
/// 每轮门检锁定/启动失败期间空转（占位连接不是业务库）；拿不到连接锁跳过本轮；
/// 失败静默记录，等下一轮重试。
pub fn start_sync_scheduler<R: Runtime>(app: &AppHandle<R>) {
    start_sync_scheduler_with(app, TriggerTimings::default());
}

/// 调度线程拉起（显式时机参数版，[`start_sync_scheduler`] 的接缝本体）：轮询
/// 周期与写后去抖窗口由调用方给定——生产传 [`TriggerTimings::default`]，触发
/// 接线测试注入「短去抖 + 超长轮询」（写后触发断言只等去抖窗，不被低频轮询
/// 救活，issue #959）。其余语义同 [`start_sync_scheduler`]。
pub fn start_sync_scheduler_with<R: Runtime>(app: &AppHandle<R>, timings: TriggerTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    if WRITE_SIGNAL.set(tx).is_err() {
        // 已有发送端（理论不可能：单次拉起守卫）：退化为纯轮询。
        tracing::warn!("同步调度已存在写信号通道，写后触发退化为低频轮询");
    }
    let conn = Arc::clone(&app.state::<DbState>().conn);
    let gate = EncryptionGate::clone(&app.state::<EncryptionGate>());
    let boot_gate = BootFailureGate::clone(&app.state::<BootFailureGate>());
    let handle = app.clone();
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
            if gate.is_locked() || boot_gate.is_failed() {
                continue;
            }
            let Some(guard) = crate::backup::lock_conn_with_timeout(&conn) else {
                continue;
            };
            run_auto_round_with_emit(&handle, &guard, &SessionEnvelope::current());
        }
    });
}

/// 自动轮次 + 失效信号：本轮实际应用外来 op（`applied > 0`）即广播参考失效
/// （与手动同步同一映射身份与证据，见 `signals::signals_for` 的 `SyncRound` 行）
/// ——同步轮次落地的数据同样要让各视图重拉，否则「同步了但界面不动」。
///
/// 失败静默（ADR-0091：同步失败不阻塞本地记账），记日志等下一轮。
fn run_auto_round_with_emit<R: Runtime>(
    app: &AppHandle<R>,
    conn: &Connection,
    session: &SessionEnvelope,
) {
    match run_auto_round(conn, session) {
        Ok(Some(report)) => {
            tracing::debug!(
                applied = report.applied,
                parked = report.parked,
                "同步轮次完成"
            );
            emit_for(
                app,
                WriteOp::SyncRound,
                WriteEvidence::LedgerApplied(report.applied > 0),
            );
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %e, "同步轮次失败（静默，等下一轮）"),
    }
}

/// 通道未配置的码化错误（手动「立即同步」入口；自动入口静默跳过）。
pub fn not_configured_error() -> AppError {
    AppError::coded(
        "sync-channel.not-configured",
        "同步通道尚未配置，请先在设置中填写网盘信息",
    )
}

/// 账本注册表不可用的码化错误（手动入口的守卫）。
pub fn book_unavailable_error() -> AppError {
    AppError::coded(
        "sync-channel.book-unavailable",
        "账本注册表不可用，无法确定同步范围",
    )
}

#[cfg(test)]
mod tests;
