use serde::{Deserialize, Serialize};

use super::command::DomainCommand;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncOp {
    pub op_id: String,
    pub device_id: String,
    pub clock: i64,
    pub schema_version: i64,
    pub command: DomainCommand,
}
