use serde::{Deserialize, Serialize};

use ledger_infra::db::query::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub insurer_id: String,
    pub policy_number: String,
    pub product_name: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub coverage_amount_cents: Option<i64>,
    pub coverage_currency_code: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

impl FromRow for Policy {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Policy {
            id: row.get("id")?,
            insurer_id: row.get("insurer_id")?,
            policy_number: row.get("policy_number")?,
            product_name: row.get("product_name")?,
            start_date: row.get("start_date")?,
            end_date: row.get("end_date")?,
            coverage_amount_cents: row.get("coverage_amount_cents")?,
            coverage_currency_code: row.get("coverage_currency_code")?,
            note: row.get("note")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            version: row.get("version")?,
            device_id: row.get("device_id")?,
            is_deleted: row.get::<_, i64>("is_deleted")? != 0,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolicyInput {
    pub insurer_id: String,
    pub policy_number: String,
    pub product_name: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub coverage_amount_cents: Option<i64>,
    pub coverage_currency_code: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PolicySourceDisplay {
    pub id: String,
    pub product_name: String,
    pub is_deleted: bool,
}

impl FromRow for PolicySourceDisplay {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicySourceDisplay {
            id: row.get(0)?,
            product_name: row.get(1)?,
            is_deleted: row.get::<_, i64>(2)? != 0,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyStats {
    pub policy_id: String,
    pub native_currency: String,
    pub total_paid_native_cents: i64,
    pub total_inflow_native_cents: i64,
    pub next_charge_date: Option<String>,
    pub is_expired: bool,
}
