use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use ledger_sync_protocol::command::SyncCommand;

use super::fields::{ConvertCommandFields, InvestmentCommandFields, SplitCommandFields};
use crate::model::NormalizedTransaction;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TransactionCommand {
    Create {
        id: String,
        row: NormalizedTransaction,
        investment: Option<InvestmentCommandFields>,
        #[serde(default)]
        convert: Option<ConvertCommandFields>,
        #[serde(default)]
        split: Option<SplitCommandFields>,
    },
    Update {
        id: String,
        row: NormalizedTransaction,
        investment: Option<InvestmentCommandFields>,
        #[serde(default)]
        convert: Option<ConvertCommandFields>,
        #[serde(default)]
        split: Option<SplitCommandFields>,
    },
    Delete {
        id: String,
    },
}

impl TransactionCommand {
    pub(crate) fn subject_id(&self) -> &str {
        match self {
            TransactionCommand::Create { id, .. } | TransactionCommand::Update { id, .. } => id,
            TransactionCommand::Delete { id } => id,
        }
    }
}

impl SyncCommand for TransactionCommand {
    const ENTITY: &'static str = "transaction";

    fn subject(&self) -> Option<Cow<'_, str>> {
        Some(Cow::Borrowed(self.subject_id()))
    }
}
