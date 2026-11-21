use super::evidence::WriteEvidence;
use super::mapping::{Signal, signals_for};
use super::write_op::WriteOp;
use crate::events;

pub fn emit_all(emitter: &dyn events::SignalEmitter, signals: &[Signal]) {
    for signal in signals {
        match signal {
            Signal::LedgerChanged => emitter.post(events::LEDGER_CHANGED),
            Signal::PricesChanged => emitter.post(events::PRICES_CHANGED),
            Signal::BackupsChanged => emitter.post(events::BACKUPS_CHANGED),
        }
    }
}

pub fn emit_for(emitter: &dyn events::SignalEmitter, op: WriteOp, evidence: WriteEvidence) {
    emit_all(emitter, signals_for(op, evidence));
}
