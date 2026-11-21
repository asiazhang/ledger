use std::time::Instant;

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::amount::TransactionKind;
use crate::model::{CreateTransactionResult, TransactionInput};
use crate::write::protocol::create as create_transaction_internal;
use ledger_infra::db::tx_scope::hold_transaction;
use ledger_infra::error::{AppError, ErrClass, Result};
use ledger_infra::signals::WriteEvidence;

#[derive(Debug)]
pub struct BatchOutcome {
    pub results: Vec<CreateTransactionResult>,
    pub evidence: WriteEvidence,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionBatch;

impl TransactionBatch {
    pub fn run(
        conn: &Connection,
        inputs: Vec<TransactionInput>,
        dedup: bool,
    ) -> Result<BatchOutcome> {
        let started = Instant::now();
        let total = inputs.len();
        let mut failed = 0usize;
        let outcome = hold_transaction(conn, || {
            let mut results = Vec::with_capacity(total);
            let mut any_merchant_created = false;
            for input in inputs {
                let idempotency_key = input.idempotency_key.clone();
                let dedup_hash = if dedup {
                    match dedup_identity(conn, &input)? {
                        DedupIdentity::Existing { id } => {
                            results.push(CreateTransactionResult {
                                success: true,
                                duplicate: true,
                                id,
                                error: None,
                            });
                            continue;
                        }
                        DedupIdentity::New { dedup_hash } => dedup_hash,
                    }
                } else {
                    compute_dedup_hash(&input)
                };
                match create_transaction_internal(conn, input) {
                    Ok(write) => {
                        any_merchant_created |= write.evidence.merchant_created();
                        if let Err(e) = conn.execute(
                            "UPDATE transactions SET dedup_hash=?1, idempotency_key=?2 WHERE id=?3",
                            rusqlite::params![dedup_hash, idempotency_key, write.id],
                        ) {
                            failed += 1;
                            return Err(e.into());
                        }
                        results.push(CreateTransactionResult {
                            success: true,
                            duplicate: false,
                            id: Some(write.id),
                            error: None,
                        });
                    }
                    Err(AppError::Invalid(msg)) => {
                        failed += 1;
                        results.push(CreateTransactionResult {
                            success: false,
                            duplicate: false,
                            id: None,
                            error: Some(msg),
                        });
                    }
                    Err(AppError::Coded {
                        class: ErrClass::Invalid,
                        message,
                        ..
                    }) => {
                        failed += 1;
                        results.push(CreateTransactionResult {
                            success: false,
                            duplicate: false,
                            id: None,
                            error: Some(message),
                        });
                    }
                    Err(e) => {
                        failed += 1;
                        return Err(e);
                    }
                }
            }
            Ok(BatchOutcome {
                results,
                evidence: WriteEvidence::MerchantCreated(any_merchant_created),
            })
        });
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(e) => {
                log_batch_summary(started, total, failed, false);
                return Err(e);
            }
        };
        if let Err(e) = conn.execute("PRAGMA optimize", []) {
            tracing::warn!(error = %e, "批量导入后 PRAGMA optimize 失败（忽略）");
        }
        log_batch_summary(started, total, failed, true);
        Ok(outcome)
    }
}

pub fn compute_dedup_hash(input: &TransactionInput) -> String {
    let to_account_id = input.to_account_id.as_deref().unwrap_or("");
    let mut payload = format!(
        "{}|{}|{}|{}|{}|{}",
        input.date,
        input.kind.as_str(),
        input.amount_cents,
        input.currency_code,
        input.account_id.as_deref().unwrap_or(""),
        to_account_id
    );
    if let Some(funding) = input.funding_account_id.as_deref() {
        payload.push('|');
        payload.push_str(funding);
    }
    if input.kind == TransactionKind::Convert {
        payload.push('|');
        payload.push_str(input.to_instrument_id.as_deref().unwrap_or(""));
        payload.push('|');
        payload.push_str(&input.to_quantity.unwrap_or(0.0).to_string());
        payload.push('|');
        payload.push_str(&input.out_amount_cents.unwrap_or(0).to_string());
        payload.push('|');
        payload.push_str(&input.in_amount_cents.unwrap_or(0).to_string());
    }
    if input.kind == TransactionKind::Dividend {
        payload.push('|');
        payload.push_str(input.instrument_id.as_deref().unwrap_or(""));
    }
    let digest = Sha256::digest(payload.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DedupIdentity {
    New { dedup_hash: String },
    Existing { id: Option<String> },
}

pub fn dedup_identity(conn: &Connection, input: &TransactionInput) -> Result<DedupIdentity> {
    let dedup_hash = compute_dedup_hash(input);
    if let Some(key) = &input.idempotency_key {
        let hit: Option<String> = conn
            .query_row(
                "SELECT id FROM transactions \
                 WHERE idempotency_key=?1 AND is_deleted=0 LIMIT 1",
                rusqlite::params![key],
                |r| r.get(0),
            )
            .optional()?;
        return Ok(match hit {
            Some(id) => DedupIdentity::Existing { id: Some(id) },
            None => DedupIdentity::New { dedup_hash },
        });
    }
    let hit: Option<String> = conn
        .query_row(
            "SELECT id FROM transactions \
             WHERE dedup_hash=?1 AND is_deleted=0 ORDER BY created_at LIMIT 1",
            rusqlite::params![dedup_hash],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match hit {
        Some(_) => DedupIdentity::Existing { id: None },
        None => DedupIdentity::New { dedup_hash },
    })
}

fn log_batch_summary(started: Instant, total: usize, failed: usize, committed: bool) {
    let msg = if committed {
        "导入批次完成"
    } else {
        "导入批次回滚"
    };
    tracing::info!(
        total,
        failed,
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
        msg
    );
}

#[cfg(test)]
mod tests;
