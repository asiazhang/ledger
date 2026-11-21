use serde::{Deserialize, Serialize};

use ledger_infra::db::query::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    InUse,
    Disposed,
}

impl ItemStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            ItemStatus::InUse => "in_use",
            ItemStatus::Disposed => "disposed",
        }
    }

    pub fn parse(s: &str) -> Result<ItemStatus, String> {
        match s {
            "in_use" => Ok(ItemStatus::InUse),
            "disposed" => Ok(ItemStatus::Disposed),
            other => Err(format!("未知物品状态: {other}（合法值: in_use/disposed）")),
        }
    }
}

impl Serialize for ItemStatus {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ItemStatus {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ItemStatus::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl rusqlite::types::FromSql for ItemStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        ItemStatus::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub purchase_date: String,
    pub total_cost_cents: i64,
    pub currency_code: String,
    pub cost_native_cents: i64,
    pub status: ItemStatus,
    pub disposal_date: Option<String>,
    pub residual_value_cents: Option<i64>,
    pub purchase_transaction_id: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

impl FromRow for Item {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Item {
            id: row.get("id")?,
            name: row.get("name")?,
            purchase_date: row.get("purchase_date")?,
            total_cost_cents: row.get("total_cost_cents")?,
            currency_code: row.get("currency_code")?,
            cost_native_cents: row.get("cost_native_cents")?,
            status: row.get("status")?,
            disposal_date: row.get("disposal_date")?,
            residual_value_cents: row.get("residual_value_cents")?,
            purchase_transaction_id: row.get("purchase_transaction_id")?,
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
pub struct ItemInput {
    pub name: String,
    pub purchase_date: String,
    pub total_cost_cents: i64,
    pub currency_code: String,
    pub note: Option<String>,
    pub purchase_transaction_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ItemDisposeInput {
    pub disposal_date: String,
    pub residual_value_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemWithDailyCost {
    #[serde(flatten)]
    pub item: Item,
    pub used_days: i64,
    pub numerator_cents: i64,
    pub per_day_cents: f64,
}

#[derive(Debug, Clone)]
pub struct ItemSourceDisplay {
    pub id: String,
    pub purchase_transaction_id: Option<String>,
    pub name: String,
    pub is_disposed: bool,
}

impl FromRow for ItemSourceDisplay {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ItemSourceDisplay {
            id: row.get(0)?,
            purchase_transaction_id: row.get(1)?,
            name: row.get(2)?,
            is_disposed: row.get::<_, String>(3)? == ItemStatus::Disposed.as_str(),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ItemDailyCost {
    pub used_days: i64,
    pub numerator_cents: i64,
    pub per_day_cents: f64,
}
