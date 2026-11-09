//! 行情同步车道宿主面（issue #2079，NSH 切轨④ / spec #2075）：market-sync 三条
//! 后台车道（价格历史补全 / 每日现价刷新 / 每日汇率增量同步）在 NSH 壳的启动
//! 接线与反馈桥。
//!
//! **启动接线**：[`crate::app::run_startup`] 成功分支单点调用
//! [`start_market_lanes`]——三条车道一并挂载（旧壳 `start_background_services`
//! 的挂载闭集平价：价格历史补全属行情数据供给面，与两条日更车道同一接线点；
//! 旧壳 lib.rs:153-155 三行 = 本处一次接线）。域侧只经公开宿主面注册点接线
//! （[`ledger_market_sync::LaneHost`]，ADR-0112 决策 5：下层定义注册点、上层
//! 注册实现、壳层启动时接线），不进域内部；车道启动与节奏沿用域既有语义
//! （生产默认 `LaneTimings`，进程级单次拉起守卫住域内各车道入口）。
//!
//! **宿主件装配**（旧壳 AppHandle 解析件的 NSH 等价物，逐件平价）：
//! - 门检：旧壳「不锁定且未失败」（两扇门判定占位连接期）；NSH 无占位连接
//!   形态——账本会话在场即门开（引导失败无会话，车道轮次空转等下一窗口）；
//! - 执行器：旧壳 `tauri::async_runtime::spawn`（tauri 全局运行时）；NSH =
//!   车道线程自持 tokio 当前线程运行时（ADR-0125 决策 7 的非 tauri 等价形：
//!   挂异步运行时的 async 任务 + `tokio::time::sleep` 异步定时，退出随进程
//!   硬停）；
//! - 会话解析：旧壳 `app.state::<DbState>()`；NSH = 会话槽逐轮解析（会话可在
//!   启动后消失，不缓存句柄）；
//! - 通道束注入槽：旧壳 `try_state` 槽位类型；NSH = 进程级注入槽（headless
//!   测试接缝，生产恒空 → 域内每轮建生产束兑底）；
//! - 进度发射与价格失效广播：经反馈桥投引擎线程（见下）。
//!
//! **反馈桥**：车道任务跑在自有线程，而进度（接缝进度信号）与投资失效广播
//! （信号订阅者表）的消费面在引擎线程（[`crate::signals`] 线程契约）——车道
//! 线程把反馈投进程级无界队列（非阻塞交接即返回，进度发射器契约同源），引擎
//! 线程泵任务（App 根组件装配，根实例所有 = 进程寿命）排空：现价进度覆写
//! [`crate::investment_sync`] 接缝进度信号（旧壳前端「只在手动同步在途时消费」
//! 的同形：呈现位只在在途时渲染，后台推进不点亮进度条、在途时两源同道覆写）；
//! 历史补全静默计数无本壳消费面（旧壳走势空态判据读路径未迁移），排空即弃；
//! 实际写入广播 [`crate::signals::notify_investments_invalidated`]（#2046 同款
//! 投影：行情写入 → 投资族信号，持仓 / 过期提示重拉）。fx 车道可重建缓存，
//! 无反馈（域内裁决面同口径）。
//!
//! **测试接缝**（`#[doc(hidden)]`，生产不消费）：复位（挂
//! [`crate::app::reset_book_assembly`] 与 lib 装配串行锁两个复位点：解除武装 +
//! 清注入槽 + 清反馈对）、武装、时机注入（`LaneTimings` 短时机）、通道束注入
//! 与反馈排空——车道接线测试（`tests/market_{price,fx,backfill}_lane.rs`）在
//! 独立进程现场注入短时机与门控桩束（根包车道集成测试同形：单次拉起守卫是
//! 进程级单例），车道反馈探针（本模块 `probe`）走真 App 装配路径。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, Ordering},
};

use ledger_market_sync::{
    FxSyncChannels, LaneHost, LaneTask, LaneTimings, SyncFetchChannels, SyncProgress,
};
use nsh_core::prelude::*;
use tokio::sync::mpsc;

use crate::{app, investment_sync, signals};

// ---------------------------------------------------------------------------
// 进程级车道状态（武装位 / 时机注入 / 通道束注入槽）
// ---------------------------------------------------------------------------

/// 车道武装位：生产默认武装；测试复位点解除武装后，`run_startup` 成功分支的
/// 接线即空转（不消费域内单次拉起守卫），已挂载车道的每轮门检也判假（场景
/// 结束后挂载线程静默空转，不触碰后续场景的库）。
static LANES_ENABLED: AtomicBool = AtomicBool::new(true);

/// 测试时机注入槽（`Some` = 一次性覆写生产默认，接线测试注入短时机后由接线
/// 取走；生产恒空走 [`LaneTimings::default`]）。
static TEST_TIMINGS: Mutex<Option<LaneTimings>> = Mutex::new(None);

/// 每日现价刷新通道束注入槽（生产恒空 = 域内每轮建生产束；headless 测试注入
/// 门控桩束使「车道真实在途」可确定复现，根包槽位类型同形）。
static PRICE_REFRESH_CHANNELS: Mutex<Option<Arc<tokio::sync::Mutex<SyncFetchChannels>>>> =
    Mutex::new(None);

/// 价格历史补全通道束注入槽（同上）。
static HISTORY_BACKFILL_CHANNELS: Mutex<Option<Arc<tokio::sync::Mutex<SyncFetchChannels>>>> =
    Mutex::new(None);

/// 每日汇率同步通道束注入槽（同上；汇率束与同步束类型不同，分槽注入）。
static FX_SYNC_CHANNELS: Mutex<Option<Arc<tokio::sync::Mutex<FxSyncChannels>>>> = Mutex::new(None);

fn test_timings_slot() -> MutexGuard<'static, Option<LaneTimings>> {
    TEST_TIMINGS.lock().unwrap_or_else(|p| p.into_inner())
}

fn price_refresh_channels_slot()
-> MutexGuard<'static, Option<Arc<tokio::sync::Mutex<SyncFetchChannels>>>> {
    PRICE_REFRESH_CHANNELS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn history_backfill_channels_slot()
-> MutexGuard<'static, Option<Arc<tokio::sync::Mutex<SyncFetchChannels>>>> {
    HISTORY_BACKFILL_CHANNELS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn fx_sync_channels_slot() -> MutexGuard<'static, Option<Arc<tokio::sync::Mutex<FxSyncChannels>>>> {
    FX_SYNC_CHANNELS.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------
// 反馈桥：车道线程 → 进程级无界队列 → 引擎线程泵
// ---------------------------------------------------------------------------

/// 车道反馈闭集（车道线程投递、引擎线程消费的三件：现价进度 / 补全静默计数 /
/// 实际写入）。Debug 供接线测试断言排空物。
#[derive(Debug)]
pub enum LaneFeedback {
    /// 现价刷新进度（`ledger:instrument-sync-progress` 事件面的桥接形态）。
    PriceProgress(SyncProgress),
    /// 价格历史补全静默计数（`ledger:history-backfill-progress` 事件面的桥接
    /// 形态；本壳无消费面，排空即弃）。
    BackfillProgress(SyncProgress),
    /// 行情实际写入（旧壳 `emit_for(SyncInstrumentInfo, PriceWritten)` 的桥接
    /// 形态；引擎线程侧广播投资族失效信号）。
    PriceWritten,
}

/// 反馈队列槽类型：`Some((发送端, 接收端))`——接收端被泵取走后置 `None`
///（同一队列只装一个泵；headless 多场景经 [`reset_for_tests`] 整对复位）。
type FeedbackSlot = (
    mpsc::UnboundedSender<LaneFeedback>,
    Option<mpsc::UnboundedReceiver<LaneFeedback>>,
);

/// 反馈队列静态槽。发送端进程级长存（车道线程随时可投）。
static FEEDBACK: Mutex<Option<FeedbackSlot>> = Mutex::new(None);

fn feedback_slot() -> MutexGuard<'static, Option<FeedbackSlot>> {
    FEEDBACK.lock().unwrap_or_else(|p| p.into_inner())
}

/// 反馈队列发送端的取用（槽空则建对装回；借用分行避免 match 内写槽）。
fn feedback_sender_clone(slot: &mut Option<FeedbackSlot>) -> mpsc::UnboundedSender<LaneFeedback> {
    if let Some((sender, _)) = slot.as_ref() {
        return sender.clone();
    }
    let (sender, receiver) = mpsc::unbounded_channel();
    *slot = Some((sender.clone(), Some(receiver)));
    sender
}

/// 投递一件反馈（非阻塞交接即返回；泵不在或队列关闭即静默丢弃——呈现面失败
/// 不影响同步结果，进度发射器契约同源）。
fn send_feedback(feedback: LaneFeedback) {
    let mut slot = feedback_slot();
    let sender = feedback_sender_clone(&mut slot);
    drop(slot);
    // 队列容量无界（车道分钟级频率，量级恒小）；发送失败（泵侧整对被复位）
    // 静默丢弃——反馈是呈现面，不回压写路径。
    let _ = sender.send(feedback);
}

/// 排空引擎线程泵的单件反馈（进度进接缝信号、写入广播投资失效；补全静默
/// 计数无本壳消费面即弃）。lib 探针直达本函数断言桥接半边（删除即红）。
fn apply_feedback(feedback: LaneFeedback) {
    match feedback {
        LaneFeedback::PriceProgress(progress) => {
            // 后台推进不点亮进度条（旧壳前端消费口径的同形）：接缝进度信号与
            // 手动同步同一对，呈现位只在手动同步在途时渲染；在途并发时两源
            // 同道覆写（旧壳同事件名同语义）。
            let seam = investment_sync::seam();
            seam.progress_done.set(progress.done as u32);
            seam.progress_total.set(progress.total as u32);
        }
        // 静默计数面的旧壳消费方（走势空态判据）读路径未迁移：无订阅者即弃，
        // 事件面语义由域内保持（本桥不另立第二套计数面）。
        LaneFeedback::BackfillProgress(_) => {}
        // 实际写入（成败同判的收尾裁决半边）→ 投资族失效信号：持仓 / 过期
        // 提示重拉（#2046 投影同款；删除本臂即车道反馈探针红）。
        LaneFeedback::PriceWritten => signals::notify_investments_invalidated(),
    }
}

/// 装配引擎线程反馈泵（App 根组件单点调用：引擎线程 + 根实例所有 = 进程
/// 寿命）。重复调用与多场景复位后再装皆幂等——接收端只被首个泵取走。
pub(crate) fn install_feedback_pump() {
    let receiver = {
        let mut slot = feedback_slot();
        let (_, receiver) = slot.get_or_insert_with(|| {
            let (sender, receiver) = mpsc::unbounded_channel();
            (sender, Some(receiver))
        });
        receiver.take()
    };
    let Some(mut receiver) = receiver else {
        return; // 泵已装配（本场景重复调用 / 复位前的旧对已装）
    };
    // 引擎线程任务：await 无界通道（车道线程唤醒跨线程拉起引擎排空，与启动
    // 任务 await blocking 同机制）；apply_feedback 的信号写在引擎线程（线程
    // 契约）。
    spawn(async move {
        while let Some(feedback) = receiver.recv().await {
            apply_feedback(feedback);
        }
    });
}

// ---------------------------------------------------------------------------
// 启动接线（run_startup 成功分支单点）
// ---------------------------------------------------------------------------

/// 行情车道任务执行器（[`LaneHost::spawn`] 宿主实现）：车道线程自持 tokio
/// 当前线程运行时驱动 async 任务——编排取数腿与会话作业需要 tokio IO / 定时
/// 器资源；车道低频（分钟级巡检 + 每自然日一轮），三车道各一线程，退出随进程
/// 硬停（写入全是幂等 upsert，中断无残留——域内核注释同口径）。
fn spawn_lane_task(task: LaneTask) {
    let thread = std::thread::Builder::new()
        .name("nsh-market-lane".to_owned())
        .spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(task),
                Err(error) => tracing::warn!(
                    %error,
                    "车道任务运行时构建失败，车道退出（进程退出前不再巡检）"
                ),
            }
        });
    if let Err(error) = thread {
        tracing::warn!(%error, "车道任务线程拉起失败，车道退出（进程退出前不再巡检）");
    }
}

/// 行情车道启动接线（[`crate::app::run_startup`] 成功分支单点调用，issue
/// #2079）：三条车道一并挂载——挂载闭集 = 旧壳 `start_background_services` 的
/// 三条行情车道（价格历史补全 + 每日现价刷新 + 每日汇率增量同步，lib.rs
/// 153-155 三行的本壳等价闭集）。武装位判假即整组空转（测试复位点语义，生产
/// 恒武装）；域内单次拉起守卫使原位重复调用幂等（ADR-0080）。
pub(crate) fn start_market_lanes() {
    if !LANES_ENABLED.load(Ordering::SeqCst) {
        return;
    }
    let timings = test_timings_slot().take().unwrap_or_default();
    let host = LaneHost {
        ready: Arc::new(|| LANES_ENABLED.load(Ordering::SeqCst) && app::db_slot().is_some()),
        spawn: Arc::new(spawn_lane_task),
        session: Arc::new(|| app::db_slot().as_ref().map(|db| db.write_handle())),
        refresh_channels: Arc::new(|| price_refresh_channels_slot().clone()),
        backfill_channels: Arc::new(|| history_backfill_channels_slot().clone()),
        fx_channels: Arc::new(|| fx_sync_channels_slot().clone()),
        refresh_progress: Arc::new(|progress| send_feedback(LaneFeedback::PriceProgress(progress))),
        backfill_progress: Arc::new(|progress| {
            send_feedback(LaneFeedback::BackfillProgress(progress))
        }),
        price_written: Arc::new(|| send_feedback(LaneFeedback::PriceWritten)),
    };
    ledger_market_sync::start_history_backfill_for_host(&host, timings);
    ledger_market_sync::start_daily_price_refresh_for_host(&host, timings);
    ledger_market_sync::start_daily_fx_sync_for_host(&host, timings);
}

// ---------------------------------------------------------------------------
// 测试接缝（#[doc(hidden)]，生产不消费）
// ---------------------------------------------------------------------------

/// 复位车道测试现场（headless / lib 测试复位点消费：解除武装 + 清时机与注入
/// 槽 + 清反馈对）。生产 bin 不消费。
#[doc(hidden)]
pub fn reset_for_tests() {
    LANES_ENABLED.store(false, Ordering::SeqCst);
    *test_timings_slot() = None;
    *price_refresh_channels_slot() = None;
    *history_backfill_channels_slot() = None;
    *fx_sync_channels_slot() = None;
    *feedback_slot() = None;
}

/// 武装车道（车道接线测试的独立进程现场：复位后显式武装再驱动启动接线）。
/// 生产 bin 不消费（默认武装）。
#[doc(hidden)]
pub fn enable_for_tests() {
    LANES_ENABLED.store(true, Ordering::SeqCst);
}

/// 注入调度时机（接线测试短时机；接线消费即取走，一次性）。
#[doc(hidden)]
pub fn set_test_timings(timings: LaneTimings) {
    *test_timings_slot() = Some(timings);
}

/// 注入每日现价刷新通道束（门控桩束——「车道真实在途」可确定复现）。
#[doc(hidden)]
pub fn inject_price_refresh_channels_for_tests(channels: SyncFetchChannels) {
    *price_refresh_channels_slot() = Some(Arc::new(tokio::sync::Mutex::new(channels)));
}

/// 注入价格历史补全通道束（同上）。
#[doc(hidden)]
pub fn inject_history_backfill_channels_for_tests(channels: SyncFetchChannels) {
    *history_backfill_channels_slot() = Some(Arc::new(tokio::sync::Mutex::new(channels)));
}

/// 注入每日汇率同步通道束（同上）。
#[doc(hidden)]
pub fn inject_fx_channels_for_tests(channels: FxSyncChannels) {
    *fx_sync_channels_slot() = Some(Arc::new(tokio::sync::Mutex::new(channels)));
}

/// 排空反馈队列（接线测试断言车道反馈桥的投递半边：实际写入 / 进度到达；
/// 泵已装配时队列由泵消费，本函数返回空集）。生产 bin 不消费。
#[doc(hidden)]
pub fn take_feedback_for_tests() -> Vec<LaneFeedback> {
    let mut slot = feedback_slot();
    let Some((_, receiver)) = slot.as_mut() else {
        return Vec::new();
    };
    let Some(receiver) = receiver else {
        return Vec::new();
    };
    let mut drained = Vec::new();
    while let Ok(feedback) = receiver.try_recv() {
        drained.push(feedback);
    }
    drained
}

#[cfg(test)]
mod probe {
    //! 车道反馈探针（#2079；形态仿 `investments_holdings::probe`）：真装配
    //! 路径（App 根组件 → 启动任务 run_startup → 车道接线 → 门控桩阻塞 →
    //! 泵排空 → 失效广播 → staleness 提示收起）。「删除即红」双锚：删
    //! `run_startup` 成功分支的 [`super::start_market_lanes()`] 调用 → 车道不
    //! 拉起、行情不写（门控抓取点等待超时即红）；删 App 根组件的
    //! [`super::install_feedback_pump()`] 装配 → 行情已写而信号不达，提示条
    //! 不收起（末段等待超时即红）。断言对准用户可观察结果（ADR-0087）。

    use std::time::Duration;

    use ledger_market_sync::{
        BulkFetchSurfaces, FxSyncChannels, LaneTimings, QuoteItem, SyncFetchChannels,
    };
    use nsh_engine::{EngineConfig, Environment, Idiom, Os, Root, Size};
    use nsh_testing::Headless;

    use super::{
        enable_for_tests, inject_fx_channels_for_tests, inject_history_backfill_channels_for_tests,
        inject_price_refresh_channels_for_tests, set_test_timings,
    };
    use crate::app::{reset_book_assembly, set_book_dir, test_assembly_guard};

    /// 接线测试巡检周期（与注入的 `LaneTimings::window_poll` 同源，根包车道
    /// IT 同形：观察窗按它推导，不另写时长字面量）。
    const POLL: Duration = Duration::from_millis(300);

    /// 门控桩的每日现价刷新通道束：批量报价抓取点先通知「车道已在途」，等
    /// 测试放行后返回一条报价（1300.00 元）——staleness 收起只能来自车道写入
    ///（探针造数不含任何价格写入路径）。
    fn gated_price_refresh_channels(
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) -> SyncFetchChannels {
        SyncFetchChannels {
            fetch_quotes: Box::new(move |_queries| {
                entered.send(()).expect("在途通知应可送达");
                release
                    .recv_timeout(Duration::from_secs(10))
                    .expect("测试应放行车道抓取");
                Box::pin(async move {
                    Ok(vec![QuoteItem {
                        code: "600519".into(),
                        name: "贵州茅台".into(),
                        price_cents: Some(130_000),
                        price_date: None,
                    }])
                })
            }),
            fetch_kline: Box::new(|_| {
                Box::pin(async {
                    unreachable!("现价刷新不发逐只 K 线请求（issue #1377）")
                })
            }),
            fetch_nav_history: Box::new(|_| {
                Box::pin(async {
                    unreachable!("探针无基金标的，净值通道不应被触达")
                })
            }),
            fetch_fund_name: Box::new(|_| {
                Box::pin(async {
                    unreachable!("探针无基金标的，名称通道不应被触达")
                })
            }),
            confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
            bulk: BulkFetchSurfaces::absent(),
        }
    }

    /// 静默桩的价格历史补全通道束：全通道空应答——补全车道零动作零网络
    ///（生产束不会建）。
    fn silent_backfill_channels() -> SyncFetchChannels {
        SyncFetchChannels {
            fetch_quotes: Box::new(|_| Box::pin(async { Ok(vec![]) })),
            fetch_kline: Box::new(|_| Box::pin(async { Ok(vec![]) })),
            fetch_nav_history: Box::new(|_| Box::pin(async { Ok(vec![]) })),
            fetch_fund_name: Box::new(|_| Box::pin(async { Ok(String::new()) })),
            confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
            bulk: BulkFetchSurfaces::absent(),
        }
    }

    /// 静默桩的每日汇率通道束：本位币单库（无非本位币痕迹）深度已达 → 走
    /// 增量腿（返回空表零落库）；全量腿不应被触达。
    fn silent_fx_channels() -> FxSyncChannels {
        FxSyncChannels {
            fetch_full: Box::new(|| {
                Box::pin(async { unreachable!("深度未达才走全量腿，探针库无痕迹") })
            }),
            fetch_incremental: Box::new(|| Box::pin(async { Ok(vec![]) })),
        }
    }

    /// 探针造数账本（staleness 探针惯例：投资账户 + 有通道标的 + 买入 1 份，
    /// 不写初始价——持仓缺现价 = 水位缺失入选，提示条在屏）。
    fn seed_probe_book(dir: &std::path::Path) {
        let db = crate::boot::boot_ledger(dir).unwrap();
        use ledger_accounts::{AccountInput, AccountType};
        use ledger_investment::{InstrumentInput, InstrumentType};
        let invest = {
            let conn = db.conn.lock().unwrap();
            ledger_accounts::create_account(
                &conn,
                AccountInput {
                    name: "证券".to_owned(),
                    kind: AccountType::Investment,
                    currency_code: "CNY".to_owned(),
                    initial_balance_cents: Some(100_000),
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                },
            )
            .unwrap()
        };
        let instrument = {
            let conn = db.conn.lock().unwrap();
            ledger_investment::create_instrument(
                &conn,
                InstrumentInput {
                    symbol: "600519".to_owned(),
                    kind: InstrumentType::Stock,
                    name: Some("贵州茅台".to_owned()),
                    currency_code: "CNY".to_owned(),
                    market: Some("sh".to_owned()),
                },
            )
            .unwrap()
        };
        let mut buy = probe_buy_input();
        buy.account_id = Some(invest);
        buy.instrument_id = Some(instrument);
        buy.quantity = Some(1.0);
        buy.price_cents = Some(10_000);
        buy.date = crate::boot::today_string();
        let conn = db.conn.lock().unwrap();
        ledger_transaction::create(&conn, buy).unwrap();
    }

    /// 买入造数输入的最小形状（字段闭集显式传递，探针惯例同款）。
    fn probe_buy_input() -> ledger_transaction::TransactionInput {
        ledger_transaction::TransactionInput {
            kind: ledger_transaction::TransactionKind::Buy,
            amount_cents: 0,
            currency_code: "CNY".to_owned(),
            account_id: None,
            to_account_id: None,
            funding_account_id: None,
            funding: Vec::new(),
            purchases: Vec::new(),
            category_id: None,
            merchant_id: None,
            merchant_name: None,
            policy_id: None,
            refund_of_transaction_id: None,
            note: Some("买入".to_owned()),
            source_order_no: None,
            date: String::new(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: Some(0),
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            origin: None,
            fx_rate: None,
            idempotency_key: None,
        }
    }

    /// 车道写入 → 反馈泵 → 失效广播 → staleness 提示收起（全链路接线探针）。
    #[test]
    fn lane_price_write_clears_staleness_alert_through_feedback_pump() {
        let _guard = test_assembly_guard();
        reset_book_assembly();
        // 车道测试现场：武装 + 短时机 + 三车道桩束（复位点已解除武装，重装）。
        enable_for_tests();
        set_test_timings(LaneTimings {
            startup_delay: Duration::from_millis(100),
            window_poll: POLL,
        });
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        inject_price_refresh_channels_for_tests(gated_price_refresh_channels(
            entered_tx, release_rx,
        ));
        inject_history_backfill_channels_for_tests(silent_backfill_channels());
        inject_fx_channels_for_tests(silent_fx_channels());

        // 探针账本：持仓缺现价（staleness 计数面）。
        let dir = std::env::temp_dir().join(format!(
            "nsh-app-lane-probe-{}",
            ledger_infra::ids::new_uuid()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ledger.db"), b"").unwrap();
        seed_probe_book(&dir);

        // 真装配路径：App 根组件的启动任务内 run_startup 成功分支挂载车道
        //（本探针不手工调 run_startup——删接线调用本探针红）。
        set_book_dir(dir);
        let mut app = Headless::start(EngineConfig {
            viewport: Size::new(nsh_engine::Points(1024.0), nsh_engine::Points(720.0)),
            environment: Environment {
                os: Os::Linux,
                idiom: Idiom::Desktop,
                ..Environment::default()
            },
            ui_sets: vec![nsh_base::registry()],
            ..EngineConfig::new(Root::of::<crate::app::App>())
        })
        .unwrap();
        app.wait_for("id:nav-investments").unwrap();
        app.press("id:nav-investments").unwrap();
        // 持仓缺现价 → 提示条在屏（staleness 判据的造数前提）。
        app.wait_for("id:staleness-alert").unwrap();

        // 车道首轮到达门控抓取点（删 start_market_lanes() 接线则超时红）。
        entered_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("每日现价刷新应到达批量报价抓取点");
        release_tx.send(()).unwrap();

        // 泵排空 → 写入广播投资失效 → staleness 重查计数归零 → 提示收起
        //（删 App 根组件的 install_feedback_pump() 装配则信号不达，红）。
        app.wait_for_gone("id:staleness-alert", Duration::from_secs(10))
            .unwrap();
    }
}
