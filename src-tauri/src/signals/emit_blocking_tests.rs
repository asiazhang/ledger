//! 「发射不阻塞写路径」回归测试（spec #366，ADR-0054 机制层回归防线）。
//!
//! 真源事故（spec #364）：写线程同步 `app.emit` 持 `webviews_lock` 等主线程
//! 回执，与主线程 URL scheme 回调抢锁成跨线程死锁——界面永久卡死。#365 把
//! 发射改为主线程非阻塞投递；本模块在机制接缝（`events::SignalEmitter`）注入
//! **可阻塞的假发射器**，只钉外部行为：
//!
//! - 发射器阻塞期间，写路径（写库提交后的发射步骤 `signals::emit_for`）仍
//!   及时返回——不阻塞、不挂起；
//! - 放行后信号最终到达、不丢失，事件名与映射判定一致。
//!
//! 刻意**不复现真实死锁时序**（跨线程时序问题易 flaky）：假发射器只构造
//! 「发射迟迟未完成」这一外部条件（假主线程被闸门挡住），不模拟
//! `webviews_lock` 抢锁路径。所有等待都带超时上界——若发射机制退化为
//! 同步等回执，测试在超时后失败，而不是永久挂起测试进程。
//!
//! 知识层（谁发什么）由同文件 `mod tests` 的映射断言与 `signals_cross_check`
//! 交叉核对守卫，与本模块（机制层，怎么发）各守一个维度、并存不覆盖。

use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::events::{self, SignalEmitter};
use crate::signals::{WriteEvidence as E, WriteOp as Op, emit_for};

/// 单次等待的超时上界：正常路径微秒级即过；仅当发射真的阻塞了写路径
///（回归发生）时才耗满并失败。
const TIMEOUT: Duration = Duration::from_secs(5);

/// 假发射器共享态：`posted` = 已交接（`post` 已调用、待发射）；
/// `delivered` = 假主线程已「发射」；`gate_open` = 闸门放行。
#[derive(Clone, Default)]
struct Shared {
    posted: Vec<&'static str>,
    delivered: Vec<&'static str>,
    gate_open: bool,
}

/// 可阻塞的假发射器（spec #366 测试桩）：`post` 只把事件**交接**给内部
/// 「假主线程」即返回（与生产 `AppHandle` 实现的非阻塞投递约定同形）；
/// 假主线程在闸门放行前一直阻塞——模拟「发射迟迟未完成」（真实事故中即
/// 主线程被 `webviews_lock` 卡住、emit 永不返回）。放行后把已交接事件依次
/// 标记为已发射并退出。可克隆，写线程与测试各持一份。
#[derive(Clone)]
struct BlockingEmitter {
    shared: Arc<(Mutex<Shared>, Condvar)>,
}

impl BlockingEmitter {
    /// 新建闸门关闭（发射器阻塞中）的假发射器，并启动假主线程。
    fn blocked() -> Self {
        let emitter = Self {
            shared: Arc::new((Mutex::new(Shared::default()), Condvar::new())),
        };
        let worker = emitter.clone();
        thread::spawn(move || worker.run_fake_main_thread());
        emitter
    }

    /// 假主线程：模拟「主线程事件循环里执行 emit」。放行前阻塞等待；
    /// 放行后把已交接事件依次「发射」并退出（测试生命周期内单次放行）。
    fn run_fake_main_thread(&self) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        while !state.gate_open {
            state = cv.wait(state).unwrap();
        }
        let pending = std::mem::take(&mut state.posted);
        state.delivered.extend(pending);
        cv.notify_all();
    }

    /// 开闸放行（模拟主线程恢复，队首待发射的信号得以执行）。
    fn open_gate(&self) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.gate_open = true;
        cv.notify_all();
    }

    /// 已交接事件快照（不等待）。
    fn posted(&self) -> Vec<&'static str> {
        self.shared.0.lock().unwrap().posted.clone()
    }

    /// 等到有交接（带超时），返回快照——证明写路径确实把事件交给了发射器。
    fn wait_posted(&self) -> Vec<&'static str> {
        self.wait_until(|s| !s.posted.is_empty(), "发射器交接")
            .posted
            .clone()
    }

    /// 等到有「发射完成」（带超时），返回快照——断言「信号最终到达、不丢失」。
    fn wait_delivered(&self) -> Vec<&'static str> {
        self.wait_until(|s| !s.delivered.is_empty(), "信号最终到达")
            .delivered
            .clone()
    }

    /// 谓词等待（带超时上界）：条件满足即返回共享态快照，超时即 panic。
    fn wait_until(&self, pred: impl Fn(&Shared) -> bool, what: &str) -> Shared {
        let (lock, cv) = &*self.shared;
        let deadline = Instant::now() + TIMEOUT;
        let mut state = lock.lock().unwrap();
        while !pred(&state) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "等待{what}超时（{TIMEOUT:?}）");
            let (guard, _) = cv.wait_timeout(state, remaining).unwrap();
            state = guard;
        }
        state.clone()
    }
}

impl SignalEmitter for BlockingEmitter {
    /// 非阻塞交接：记录事件并唤醒假主线程后**立即返回**，不等发射完成
    ///（与生产 `AppHandle` 实现同一约定——这正是被本模块钉死的契约）。
    fn post(&self, event: &'static str) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.posted.push(event);
        cv.notify_all();
    }
}

/// 核心回归（spec #366）：发射器阻塞期间，写路径（壳层在写库提交成功后的
/// 发射步骤）仍及时返回；闸门放行后信号最终到达、事件名正确、不丢失。
#[test]
fn write_returns_while_emitter_blocked_and_signal_arrives_once_released() {
    let emitter = BlockingEmitter::blocked();

    // 模拟写线程：与生产壳层调用形态一致（写库提交成功后发射，此处只测发射步骤）。
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let writer_emitter = emitter.clone();
    let writer = thread::spawn(move || {
        emit_for(&writer_emitter, Op::CreateAccount, E::None);
        done_tx.send(()).expect("写线程回执通道存活");
    });

    // 前置：CreateAccount 的信号已交接给发射器（ledger:changed），且仍未发射
    // 完成——闸门未放行，假主线程（发射）阻塞中。
    assert_eq!(emitter.wait_posted(), vec![events::LEDGER_CHANGED]);

    // 关键断言：发射器阻塞期间写路径已经返回——不阻塞、不挂起。
    // 若发射机制退化为「同步等回执」，此处耗满超时后失败（而非挂起测试进程）。
    done_rx
        .recv_timeout(TIMEOUT)
        .expect("写路径被发射器阻塞：emit 应回收交接即返回，不等发射完成");

    // 放行后信号最终到达（不允许丢失），且恰为交接的那一条。
    emitter.open_gate();
    assert_eq!(emitter.wait_delivered(), vec![events::LEDGER_CHANGED]);

    writer.join().expect("写线程正常结束");
}

/// 零信号写操作不触碰发射器：即便发射器阻塞，预算写（刻意零信号行）也
/// 不产生任何交接——「不发」在映射行显式可查（ADR-0044），壳层零投递。
#[test]
fn zero_signal_write_leaves_blocked_emitter_untouched() {
    let emitter = BlockingEmitter::blocked();

    emit_for(&emitter, Op::CreateBudget, E::None);

    assert!(
        emitter.posted().is_empty(),
        "零信号写操作不得触碰发射器，实际交接了 {:?}",
        emitter.posted()
    );

    // 开闸让假主线程退出，不留永久阻塞线程（测试卫生）。
    emitter.open_gate();
}
