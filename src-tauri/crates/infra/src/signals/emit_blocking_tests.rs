//! 「发射不阻塞写路径」回归测试（spec #366，ADR-0054 机制层回归防线）。
//!
//! 真源事故（spec #364）：写线程同步 `app.emit` 持 `webviews_lock` 等主线程
//! 回执，与主线程 URL scheme 回调抢锁成跨线程死锁——界面永久卡死。#365 把
//! 发射改为主线程非阻塞投递；本模块在机制接缝（`events::SignalEmitter`）注入
//! **闸门式假发射器**（共享测试桩 `test_utils::GatedEmitter`），只钉外部行为：
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
//! 交叉核对守卫，与本模块（机制层，怎么发）各守一个维度、并存不覆盖；
//! HTTP 壳整链维度（真端点 + 发射槽）由 `tests/api_server/signal_delivery.rs`
//!（spec #367）守卫，与本模块共用同一假发射器。

use std::sync::mpsc;
use std::thread;

use crate::events;
use crate::signals::{WriteEvidence as E, WriteOp as Op, emit_for};
use crate::test_utils::{GATED_TIMEOUT, GatedEmitter};

/// 核心回归（spec #366）：发射器阻塞期间，写路径（壳层在写库提交成功后的
/// 发射步骤）仍及时返回；闸门放行后信号最终到达、事件名正确、不丢失。
#[test]
fn write_returns_while_emitter_blocked_and_signal_arrives_once_released() {
    let emitter = GatedEmitter::gated();

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
        .recv_timeout(GATED_TIMEOUT)
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
    let emitter = GatedEmitter::gated();

    emit_for(&emitter, Op::CreateBudget, E::None);

    assert!(
        emitter.posted().is_empty(),
        "零信号写操作不得触碰发射器，实际交接了 {:?}",
        emitter.posted()
    );

    // 开闸让假主线程退出，不留永久阻塞线程（测试卫生）。
    emitter.open_gate();
}
