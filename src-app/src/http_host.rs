//! NSH 侧 AI 导入 HTTP 壳挂载与失效信号泵（#2081）：`api_server` 迁居本壳后的
//! 生产接线面——旧壳 `lib.rs` setup 内 `start_http_server` 挂载的本壳对应物。
//!
//! tauri 依赖反转（ADR-0112 决策 5：下层定义注册点、上层注册实现、壳层启动时
//! 接线）：旧壳把 `tauri::AppHandle`（SignalEmitter 的 tauri-emit 生产实现）
//! 直传 `start_http_server`；本壳零 tauri 依赖（#2083 切轨后旧壳退役，全仓
//! 无 tauri 门），api_server 面向既有抽象
//! `EmitterSlot`（`Option<Arc<dyn SignalEmitter>>`）参数化，生产发射器由本模块
//! 接线——[`NshHttpEmitter`] 把基础设施失效事件（`ledger_infra::events`）泵进
//! 壳内失效信号族（[`crate::signals`]）。
//!
//! 信号投递语义不变（`tests/api_server/signal_delivery.rs` 钉着，断言逐字节
//! 未动）：发射时序 = 写事务提交成功后经 `emit_for` 交接（非阻塞 `post`，
//! 发射失败静默忽略、不影响写结果，ADR-0044）；「发不发、发哪个」判定单点
//! 仍是 `signals_for` 映射。与旧壳的唯一机制差异在投递末端：旧壳经 Tauri 事件
//! 跨进程边界投给前端 JS store，本壳经泵任务投给引擎线程的壳内族广播
//! （[`crate::signals`] 的 thread_local 订阅者表只在引擎线程可见）——视图
//! 订阅重读的用户可观察结果同形（headless 接线断言钉住，删除泵接线即红）。
//!
//! 事件 → 族映射（粗粒度、无 payload 语义不变）：`LEDGER_CHANGED` → 三族全
//! 广播（旧壳单一 ledger 事件触发前端全部相关 store 重拉的等价形，宁多读一
//! 次不漏广播——失效模式反转 ADR-0035：漏广播的症状是「库已变而列表仍旧」，
//! 用户可见）；`PRICES_CHANGED` → 投资族（价格变 → 持仓 / 概览重读）；
//! `BACKUPS_CHANGED` → 零映射（NSH 无备份视图消费面，注册点随备份管理迁移票
//! 补齐）。
//!
//! 接线时序（启动竖井的线程拓扑裁决）：`run_startup` 在组件任务的 blocking
//! 后台线程执行（SQLite 阻塞 IO 不占引擎线程）——HTTP 服务挂载住竖井内
//! （[`mount`]，进程级一次，无引擎依赖）；泵任务需要引擎线程（壳内族广播
//! 经 thread_local 订阅者表，[`crate::signals`] 线程契约），由组件层在竖井
//! await 回引擎后接线（[`spawn_pump`]，`crate::app` 就绪分支调用）。
//!
//! 生命周期（引擎级 × 进程级两半，headless 多场景串行共存的前提裁决）：
//! - **HTTP 服务是进程级设施**（端口绑定一次、绑定失败 fail loud 与旧壳同
//!   语义）：NSH 产品进程只走一次启动竖井；headless 后续场景共享首次挂载
//!   实例——契约自举端点不触 DB，服务在位即应答。
//! - **泵任务是引擎级设施**（每次 [`spawn_pump`] 重起 + 通道槽换装）：壳内族
//!   广播只在本引擎线程有效（thread_local），headless 每场景起新引擎、串行
//!   退场——每场景给自己的引擎配一份泵，旧引擎退场即旧泵终止（通道闭合，
//!   `NshHttpEmitter::post` 静默丢弃，ADR-0044）。直调 `run_startup` 的
//!   lib 单测无引擎无视图消费面：不接线泵（服务照挂），post 静默丢弃即
//!   测试态语义。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::sync::{Arc, Mutex, OnceLock};

use ledger_infra::db::DbState;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::events::SignalEmitter;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::api_server;
use crate::signals;

/// HTTP 服务进程级挂载单次闸：端口绑定一次（见模块文档生命周期裁决）。
static HTTP_MOUNTED: OnceLock<()> = OnceLock::new();

/// 泵通道槽（进程级，互斥体内槽换装——与 ADR-0080 换连槽同形）：[`mount`]
/// 每次执行换装为当前引擎的泵通道；发射器 [`NshHttpEmitter`] 经槽投递，
/// 换装后旧通道闭合、旧泵任务随 `recv` 失败终止。
static PUMP_TX: Mutex<Option<UnboundedSender<&'static str>>> = Mutex::new(None);

/// NSH 泵发射器（SignalEmitter 的本壳生产实现，旧壳 AppHandle 的对应物）：
/// `post` 经 [`PUMP_TX`] 槽非阻塞交接即返回（与生产 `AppHandle` 实现的非阻塞
/// 投递约定同形——这正是 signal_delivery 集成测试钉住的契约）；通道闭合
///（泵任务退场）或发送失败即静默丢弃——发射失败静默、不影响写结果（ADR-0044）。
struct NshHttpEmitter;

impl SignalEmitter for NshHttpEmitter {
    fn post(&self, event: &'static str) {
        // 锁中毒 / 通道闭即静默丢弃：失效信号不承载成败语义，发射失败静默
        // 忽略、不影响写结果（ADR-0044）。
        if let Ok(guard) = PUMP_TX.lock()
            && let Some(tx) = guard.as_ref()
        {
            let _ = tx.send(event);
        }
    }
}

/// 泵任务（引擎执行器上轮询，见模块文档生命周期裁决）：把交接进来的失效事件
/// 按模块文档的映射规则广播为壳内失效信号（粗粒度、无 payload——消费方订阅
/// 后自行重拉）。
async fn pump(mut rx: UnboundedReceiver<&'static str>) {
    while let Some(event) = rx.recv().await {
        match event {
            ledger_infra::events::LEDGER_CHANGED => {
                // 三族全广播：旧壳单一 ledger 事件等价形（模块文档映射裁决）。
                signals::notify_transactions_invalidated();
                signals::notify_budgets_invalidated();
                signals::notify_investments_invalidated();
            }
            ledger_infra::events::PRICES_CHANGED => {
                signals::notify_investments_invalidated();
            }
            // BACKUPS_CHANGED 及未知事件：零映射（NSH 无备份视图消费面）。
            _ => {}
        }
    }
}

/// 挂载 AI 导入 HTTP 壳（run_startup 就绪分支的单点接线）：HTTP 服务进程级
/// 一次（[`HTTP_MOUNTED`]），泵任务引擎级每次重起（[`PUMP_TX`] 换装，模块
/// 文档生命周期裁决）。调用线程即引擎线程（只在组件任务内的
/// [`crate::app::run_startup`] 被调）：泵任务 `spawn` 进当前引擎执行器常驻。
/// 端口默认 `127.0.0.1:9527`，绑定失败 fail loud，与旧壳同语义；默认端口
/// 被占用（同机并发测试进程 / 第二实例）时回落内核分配的临时端口，实际
/// 绑定端口经 [`mounted_port`] 可观察（#2112 端口去固定化）。
///
/// 门注入：加密锁定门恒开（NSH 立桩只引导未加密账本库，[`crate::boot::
/// boot_ledger`] 拒密文库，锁定态随解锁向导迁移票补齐）；启动失败门恒未失败
/// （NSH 引导失败直接呈现失败视图、无门翻转宿主）——门实例随各自宿主迁移票
/// 换装进程级单例。
pub fn mount(db: &DbState) {
    // HTTP 服务进程级一次；本函数在启动竖井（blocking 后台线程）内被调，
    // 首次挂载的库槽即产品进程的会话槽。泵接线在引擎线程侧（[`spawn_pump`]，
    // 组件层就绪分支调用）。
    HTTP_MOUNTED.get_or_init(|| {
        let emitter: Option<Arc<dyn SignalEmitter>> = Some(Arc::new(NshHttpEmitter));
        api_server::start_http_server(
            emitter,
            db.conn.clone(),
            db.read_conn.clone(),
            EncryptionGate::new(false),
            BootFailureGate::new(),
        );
    });
}

/// 泵事件的测试注入面（headless 接线断言用）：把失效事件直送泵通道——与
/// 写端点经 `emit_for` 交接同一投递段（[`NshHttpEmitter::post`] 的通道半边）。
/// 「HTTP 写端点 → emitter.post」的端到端由 `tests/api_server/signal_delivery.rs`
/// 钉着；本注入面使 headless 能独立验证「泵 → 壳内族广播 → 视图重读」段
///（headless 场景的服务库槽可能归属首个挂载场景，写端点路径在多场景并行下
/// 不自洽——模块文档生命周期裁决）。生产零消费。
///
/// 本进程 HTTP 服务实际绑定端口（headless 接线断言消费，#2112）：`None` =
/// 尚未绑定。断言「run_startup 挂载接线」的唯一可观察面——删除挂载接线后
/// 本口永为 `None`，限时轮询超时即红（不再误连同机他进程占用的 9527 假绿）。
#[doc(hidden)]
pub fn mounted_port() -> Option<u16> {
    api_server::bound_port()
}

#[doc(hidden)]
pub fn post_invalidation_for_tests(event: &'static str) {
    // 锁失败静默返回（与 post 同语义：测试注入不承载成败）。
    if let Ok(guard) = PUMP_TX.lock()
        && let Some(tx) = guard.as_ref()
    {
        let _ = tx.send(event);
    }
}

/// 泵任务接线（引擎线程侧，组件层启动竖井就绪后调用）：通道槽换装 + 泵任务
/// spawn 进当前引擎执行器常驻——旧通道闭合、旧泵（上一引擎的）随 `recv`
/// 失败终止；引擎退场任务随之终止，通道闭合后 emitter 的 post 静默丢弃
///（发射失败静默，ADR-0044）。
pub fn spawn_pump() {
    let (tx, rx) = unbounded_channel();
    // 锁中毒：放弃换装（无泵 → post 静默丢弃，与通道闭同一降级语义）。
    if let Ok(mut slot) = PUMP_TX.lock() {
        *slot = Some(tx);
        nsh_core::element::spawn(pump(rx));
    }
}
