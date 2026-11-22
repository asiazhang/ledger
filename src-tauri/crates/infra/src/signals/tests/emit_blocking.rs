use std::sync::mpsc;
use std::thread;

use crate::events;
use crate::signals::{WriteEvidence as E, WriteOp as Op, emit_for};
use crate::test_utils::{GATED_TIMEOUT, GatedEmitter};

#[test]
fn write_returns_while_emitter_blocked_and_signal_arrives_once_released() {
    let emitter = GatedEmitter::gated();

    let (done_tx, done_rx) = mpsc::channel::<()>();
    let writer_emitter = emitter.clone();
    let writer = thread::spawn(move || {
        emit_for(&writer_emitter, Op::CreateAccount, E::None);
        done_tx.send(()).expect("写线程回执通道存活");
    });

    assert_eq!(emitter.wait_posted(), vec![events::LEDGER_CHANGED]);

    done_rx
        .recv_timeout(GATED_TIMEOUT)
        .expect("写路径被发射器阻塞：emit 应回收交接即返回，不等发射完成");

    emitter.open_gate();
    assert_eq!(emitter.wait_delivered(), vec![events::LEDGER_CHANGED]);

    writer.join().expect("写线程正常结束");
}

#[test]
fn zero_signal_write_leaves_blocked_emitter_untouched() {
    let emitter = GatedEmitter::gated();

    emit_for(&emitter, Op::CreateBudget, E::None);

    assert!(
        emitter.posted().is_empty(),
        "零信号写操作不得触碰发射器，实际交接了 {:?}",
        emitter.posted()
    );

    emitter.open_gate();
}
