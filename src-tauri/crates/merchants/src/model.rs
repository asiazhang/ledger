use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use ledger_infra::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Merchant {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MerchantInput {
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[schema(
    description = "更新入参（IPC `update_merchant` 与 HTTP `PUT /api/v1/merchants/{id}` 共用）：`name` 可省略（省略即保持原值，等价空更新）；入参先 trim（与导入即建同款归一），trim 后为空报 `merchant.name-required`，改名撞在用同名报 `merchant.already-exists`。"
)]
pub struct MerchantUpdateInput {
    #[doc = "新商户名（可省略：省略即保持原值；首尾空白由后端修剪）。"]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MerchantTransactionCount {
    pub merchant_id: String,
    pub transaction_count: i64,
}

impl FromRow for Merchant {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Merchant {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            version: row.get(4)?,
            device_id: row.get(5)?,
            is_deleted: row.get::<_, i64>(6)? != 0,
        })
    }
}

impl FromRow for MerchantTransactionCount {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(MerchantTransactionCount {
            merchant_id: row.get(0)?,
            transaction_count: row.get(1)?,
        })
    }
}
