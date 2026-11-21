use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValuationCommandRow {
    pub id: String,
    pub valuation_date: String,
    pub amount_cents: i64,
    pub currency_code: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PhysicalAssetCommand {
    Create {
        id: String,
        name: String,
        purchase_date: Option<String>,
        purchase_price_cents: Option<i64>,
        purchase_currency_code: Option<String>,
        first_valuation: ValuationCommandRow,
    },
    Update {
        id: String,
        name: String,
        purchase_date: Option<String>,
        purchase_price_cents: Option<i64>,
        purchase_currency_code: Option<String>,
    },
    UpdateValuation {
        asset_id: String,
        valuation: ValuationCommandRow,
    },
    Dispose {
        id: String,
        disposal_date: String,
        disposal_price_cents: Option<i64>,
        disposal_currency_code: Option<String>,
    },
    Delete {
        id: String,
    },
}

impl PhysicalAssetCommand {
    pub(crate) fn subject(&self) -> Option<&str> {
        match self {
            PhysicalAssetCommand::Create { id, .. }
            | PhysicalAssetCommand::Update { id, .. }
            | PhysicalAssetCommand::Dispose { id, .. }
            | PhysicalAssetCommand::Delete { id } => Some(id),
            PhysicalAssetCommand::UpdateValuation { .. } => None,
        }
    }
}

impl SyncCommand for PhysicalAssetCommand {
    const ENTITY: &'static str = "physical_asset";

    fn subject(&self) -> Option<Cow<'_, str>> {
        self.subject().map(Cow::Borrowed)
    }
}

pub(crate) fn record_local(conn: &Connection, command: PhysicalAssetCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_command(conn: &Connection, command: &PhysicalAssetCommand) -> Result<()> {
    match command {
        PhysicalAssetCommand::Create {
            id,
            name,
            purchase_date,
            purchase_price_cents,
            purchase_currency_code,
            first_valuation,
        } => super::crud::replay_create(
            conn,
            id,
            name,
            purchase_date.as_deref(),
            *purchase_price_cents,
            purchase_currency_code.as_deref(),
            first_valuation,
        ),
        PhysicalAssetCommand::Update {
            id,
            name,
            purchase_date,
            purchase_price_cents,
            purchase_currency_code,
        } => super::crud::replay_update(
            conn,
            id,
            name,
            purchase_date.as_deref(),
            *purchase_price_cents,
            purchase_currency_code.as_deref(),
        ),
        PhysicalAssetCommand::UpdateValuation {
            asset_id,
            valuation,
        } => super::crud::replay_valuation(conn, asset_id, valuation),
        PhysicalAssetCommand::Dispose {
            id,
            disposal_date,
            disposal_price_cents,
            disposal_currency_code,
        } => super::crud::replay_dispose(
            conn,
            id,
            disposal_date,
            *disposal_price_cents,
            disposal_currency_code.as_deref(),
        ),
        PhysicalAssetCommand::Delete { id } => super::crud::replay_delete(conn, id),
    }
}
