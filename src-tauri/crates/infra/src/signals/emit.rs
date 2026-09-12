//! 发射助手（ADR-0044 决策 1，机制侧收口）：把信号集逐个投递给发射器接缝。
//!
use super::evidence::WriteEvidence;
use super::mapping::{Signal, signals_for};
use super::write_op::WriteOp;
use crate::events;

/// 发射助手（ADR-0044 决策 1，机制侧收口）：遍历信号集逐个把事件名投递给发射器。
/// 发射器即机制接缝（`events::SignalEmitter`，spec #366）：生产路径传 `&AppHandle`
/// （主线程非阻塞投递，ADR-0054，trait 自动转型），测试注入闸门式假发射器
///（`test_utils::GatedEmitter`）断言「发射不阻塞写路径」（`tests::emit_blocking`，
/// spec #366）。本函数只做一次非阻塞
/// 投递即返回，不等发射完成；投递 / 发射失败静默忽略，不影响写事务结果。
/// 壳层约定形态：写路径经统一写入口 `write_entry`（ADR-0073）内化发射；
/// 本函数供不经入口的例外路径（备份修剪）与写入口本体消费，
/// 或先取 [`signals_for`] 再 [`emit_all`]（需要先记日志 / 断言信号集时用后者）。
pub fn emit_all(emitter: &dyn events::SignalEmitter, signals: &[Signal]) {
    for signal in signals {
        match signal {
            Signal::LedgerChanged => emitter.post(events::LEDGER_CHANGED),
            Signal::PricesChanged => emitter.post(events::PRICES_CHANGED),
            Signal::BackupsChanged => emitter.post(events::BACKUPS_CHANGED),
        }
    }
}

/// 组合助手：取 [`signals_for`] 判定并立即发射（写入口与例外路径的单行形态）。
pub fn emit_for(emitter: &dyn events::SignalEmitter, op: WriteOp, evidence: WriteEvidence) {
    emit_all(emitter, signals_for(op, evidence));
}
