//! 商户领域模型：商户实体与入参（issue #188 / ADR-0028）。
//!
//! 商户是参考数据字典（与分类/账户同款模式）：`name` 在用行全库唯一、
//! 软删除；交易以 `merchant_id` 引用（见核心交易域 Transaction），
//! 改名/软删经命令面收敛（见 `commands::merchants`）。
//! 商户回归「名字字典」：`icon` / `color` 已退役（issue #223）。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::query::FromRow;

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

/// 更新入参：`name` 可省略（省略即保持原值，等价空更新）；改名须避开在用同名。
#[derive(Debug, Deserialize)]
pub struct MerchantUpdateInput {
    pub name: Option<String>,
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
