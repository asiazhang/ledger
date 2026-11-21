use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use std::borrow::Cow;

use ledger_infra::db::{deterministic_uuid, now_iso};
use ledger_infra::error::Result;
use ledger_sync_protocol::command::{ReplayEffect, SyncCommand};
use ledger_sync_protocol::device::device_id;
use ledger_sync_protocol::op::record_local as record_op;
use ledger_transaction::NormalizedTransaction;
use ledger_transaction::write::writer;

use super::models::{CreateScheduledInput, ScheduledStatus, UpdateSubscriptionInput};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScheduledCommand {
    ExecuteOccurrence {
        plan_id: String,
        scheduled_date: String,
        row: NormalizedTransaction,
    },
    CreatePlan {
        id: String,
        input: CreateScheduledInput,
    },
    UpdatePlanStatus {
        id: String,
        new_status: ScheduledStatus,
    },
    UpdateSubscription {
        id: String,
        account_id: String,
        category_id: Option<String>,
        note: Option<String>,
        merchant_id: Option<String>,
    },
    ExpandOccurrences {
        plan_id: String,
    },
}

impl ScheduledCommand {
    pub fn subject(&self) -> Option<&str> {
        match self {
            ScheduledCommand::ExecuteOccurrence { .. }
            | ScheduledCommand::ExpandOccurrences { .. } => None,
            ScheduledCommand::CreatePlan { id, .. }
            | ScheduledCommand::UpdatePlanStatus { id, .. }
            | ScheduledCommand::UpdateSubscription { id, .. } => Some(id),
        }
    }

    pub fn landing_transaction_id(&self) -> String {
        match self {
            ScheduledCommand::ExecuteOccurrence {
                plan_id,
                scheduled_date,
                ..
            } => occurrence_transaction_id(plan_id, scheduled_date),
            _ => String::new(),
        }
    }
}

impl SyncCommand for ScheduledCommand {
    const ENTITY: &'static str = "scheduled";

    fn subject(&self) -> Option<Cow<'_, str>> {
        ScheduledCommand::subject(self).map(Cow::Borrowed)
    }
}

pub fn occurrence_transaction_id(plan_id: &str, scheduled_date: &str) -> String {
    deterministic_uuid(&format!("scheduled-occurrence|{plan_id}|{scheduled_date}"))
}

pub(crate) fn record_local(conn: &Connection, command: ScheduledCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub(crate) fn transaction_landed(conn: &Connection, transaction_id: &str) -> Result<bool> {
    let hit: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM transactions WHERE id = ?1",
            params![transaction_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(hit.is_some())
}

fn complete_local_occurrence(
    conn: &Connection,
    plan_id: &str,
    scheduled_date: &str,
    transaction_id: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE scheduled_transaction_occurrences \
         SET status='completed', transaction_id=?3, updated_at=?4, version=version+1, device_id=?5 \
         WHERE scheduled_transaction_id=?1 AND scheduled_date=?2 \
           AND status IN ('pending','failed') AND is_deleted=0",
        params![
            plan_id,
            scheduled_date,
            transaction_id,
            now_iso(),
            device_id(conn)?
        ],
    )?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &ScheduledCommand) -> Result<ReplayEffect> {
    match command {
        ScheduledCommand::ExecuteOccurrence {
            plan_id,
            scheduled_date,
            row,
        } => {
            let landing_id = occurrence_transaction_id(plan_id, scheduled_date);
            if transaction_landed(conn, &landing_id)? {
                return Ok(ReplayEffect::IdempotentHit);
            }
            writer::validate_accounts_alive(
                conn,
                row.account_id.as_deref(),
                row.to_account_id.as_deref(),
                None,
                &[],
            )?;
            let norm = writer::NormalizedRow::try_from(row)?;
            writer::insert_row_with_id(conn, &landing_id, &norm)?;
            complete_local_occurrence(conn, plan_id, scheduled_date, &landing_id)?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::CreatePlan { id, input } => {
            super::engine::create_protocol(conn, id, input)?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::UpdatePlanStatus { id, new_status } => {
            super::engine::update_plan_status_protocol(conn, id, *new_status)?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::UpdateSubscription {
            id,
            account_id,
            category_id,
            note,
            merchant_id,
        } => {
            super::engine::update_subscription_protocol(
                conn,
                &UpdateSubscriptionInput {
                    id: id.clone(),
                    account_id: account_id.clone(),
                    category_id: category_id.clone(),
                    note: note.clone(),
                    merchant_id: merchant_id.clone(),
                    amount_cents: false,
                    total_amount_cents: false,
                },
            )?;
            Ok(ReplayEffect::Applied)
        }
        ScheduledCommand::ExpandOccurrences { plan_id } => {
            super::engine::expand_occurrences_protocol(conn, plan_id)?;
            Ok(ReplayEffect::Applied)
        }
    }
}
