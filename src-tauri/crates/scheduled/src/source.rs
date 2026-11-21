use std::collections::HashMap;

use rusqlite::Connection;

use super::models::ScheduledKind;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::Result;
use ledger_transaction::{TransactionSource, TransactionSourceKind, TransactionSourceStatus};

#[derive(Debug, Clone)]
pub struct PlanSourceDisplay {
    pub transaction_id: String,
    pub plan_id: String,
    pub kind: ScheduledKind,
    pub status: String,
    pub note: Option<String>,
}

impl FromRow for PlanSourceDisplay {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PlanSourceDisplay {
            transaction_id: row.get(0)?,
            plan_id: row.get(1)?,
            kind: row.get(2)?,
            status: row.get(3)?,
            note: row.get(4)?,
        })
    }
}

pub fn source_display_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<PlanSourceDisplay>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    query_all(
        conn,
        &format!(
            "SELECT o.transaction_id, p.id, p.kind, p.status, p.note \
             FROM scheduled_transaction_occurrences o \
             JOIN scheduled_transactions p ON p.id = o.scheduled_transaction_id \
             WHERE o.transaction_id IN ({placeholders})"
        ),
        rusqlite::params_from_iter(transaction_ids.iter()),
    )
}

pub fn install_plan_source_hook() {
    ledger_transaction::seams::source::register_plan_source_resolver(plan_source_hook);
}

fn plan_source_hook(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<HashMap<String, TransactionSource>> {
    Ok(source_display_by_transaction_ids(conn, transaction_ids)?
        .into_iter()
        .map(|row| {
            let source = TransactionSource {
                kind: match row.kind {
                    ScheduledKind::Installment => TransactionSourceKind::InstallmentPlan,
                    ScheduledKind::Subscription => TransactionSourceKind::Subscription,
                    ScheduledKind::ScheduledTransfer => TransactionSourceKind::ScheduledTransfer,
                },
                entity_id: row.plan_id,
                display_name: row.note.unwrap_or_default(),
                status: (row.status == "cancelled").then_some(TransactionSourceStatus::Cancelled),
            };
            (row.transaction_id, source)
        })
        .collect())
}
