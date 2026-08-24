//! 汇率领域模型。

use serde::{Deserialize, Serialize};

use crate::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExchangeRate {
    pub id: String,
    pub base_code: String,
    pub quote_code: String,
    pub rate: f64,
    pub priced_at: String,
    pub source: Option<String>,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ExchangeRateInput {
    pub base_code: String,
    pub quote_code: String,
    pub rate: f64,
    pub priced_at: String,
    pub source: Option<String>,
}

impl FromRow for ExchangeRate {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ExchangeRate {
            id: row.get(0)?,
            base_code: row.get(1)?,
            quote_code: row.get(2)?,
            rate: row.get(3)?,
            priced_at: row.get(4)?,
            source: row.get(5)?,
            updated_at: row.get(6)?,
            version: row.get(7)?,
            device_id: row.get(8)?,
        })
    }
}
