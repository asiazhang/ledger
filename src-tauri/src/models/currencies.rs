//! 币种领域模型。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Currency {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i64,
}

impl FromRow for Currency {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Currency {
            code: row.get(0)?,
            name: row.get(1)?,
            symbol: row.get(2)?,
            decimal_places: row.get(3)?,
        })
    }
}
