use serde::{Deserialize, Serialize};

use ledger_infra::db::query::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalAssetStatus {
    Holding,
    Disposed,
}

impl PhysicalAssetStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            PhysicalAssetStatus::Holding => "holding",
            PhysicalAssetStatus::Disposed => "disposed",
        }
    }

    pub fn parse(s: &str) -> Result<PhysicalAssetStatus, String> {
        match s {
            "holding" => Ok(PhysicalAssetStatus::Holding),
            "disposed" => Ok(PhysicalAssetStatus::Disposed),
            other => Err(format!(
                "未知实物资产状态: {other}（合法值: holding/disposed）"
            )),
        }
    }
}

impl Serialize for PhysicalAssetStatus {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PhysicalAssetStatus {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PhysicalAssetStatus::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl rusqlite::types::FromSql for PhysicalAssetStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        PhysicalAssetStatus::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AssetRecord {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) purchase_date: Option<String>,
    pub(crate) purchase_price_cents: Option<i64>,
    pub(crate) purchase_currency_code: Option<String>,
    pub(crate) status: PhysicalAssetStatus,
    pub(crate) disposal_date: Option<String>,
    pub(crate) disposal_price_cents: Option<i64>,
    pub(crate) disposal_currency_code: Option<String>,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) version: i64,
    pub(crate) device_id: String,
    pub(crate) is_deleted: bool,
    pub(crate) current_valuation_cents: i64,
    pub(crate) current_valuation_currency_code: String,
    pub(crate) current_valuation_date: String,
}

impl FromRow for AssetRecord {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(AssetRecord {
            id: row.get("id")?,
            name: row.get("name")?,
            purchase_date: row.get("purchase_date")?,
            purchase_price_cents: row.get("purchase_price_cents")?,
            purchase_currency_code: row.get("purchase_currency_code")?,
            status: row.get("status")?,
            disposal_date: row.get("disposal_date")?,
            disposal_price_cents: row.get("disposal_price_cents")?,
            disposal_currency_code: row.get("disposal_currency_code")?,
            created_at: row.get("created_at")?,
            updated_at: row.get("updated_at")?,
            version: row.get("version")?,
            device_id: row.get("device_id")?,
            is_deleted: row.get::<_, i64>("is_deleted")? != 0,
            current_valuation_cents: row.get("current_valuation_cents")?,
            current_valuation_currency_code: row.get("current_valuation_currency_code")?,
            current_valuation_date: row.get("current_valuation_date")?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalAsset {
    pub id: String,
    pub name: String,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    pub purchase_currency_code: Option<String>,
    pub status: PhysicalAssetStatus,
    pub disposal_date: Option<String>,
    pub disposal_price_cents: Option<i64>,
    pub disposal_currency_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
    pub current_valuation_cents: i64,
    pub current_valuation_currency_code: String,
    pub current_valuation_date: String,
    pub current_valuation_native_cents: Option<i64>,
    pub native_currency: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetInput {
    pub name: String,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    pub purchase_currency_code: Option<String>,
    pub initial_valuation_cents: Option<i64>,
    pub initial_valuation_currency_code: Option<String>,
    pub initial_valuation_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetUpdateInput {
    pub name: String,
    pub purchase_date: Option<String>,
    pub purchase_price_cents: Option<i64>,
    pub purchase_currency_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetValuationInput {
    pub amount_cents: Option<i64>,
    pub currency_code: Option<String>,
    pub valuation_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetDisposeInput {
    pub disposal_date: Option<String>,
    pub disposal_price_cents: Option<i64>,
    pub disposal_currency_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PhysicalAssetList {
    pub assets: Vec<PhysicalAsset>,
    pub holding_total_native_cents: i64,
    pub native_currency: String,
}
