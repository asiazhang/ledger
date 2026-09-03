//! 商户领域模型（#419 随域归位）：商户实体与入参（issue #188 / ADR-0028）。
//!
//! 自全局模型目录迁入本域（#417 归属原则：实体归属优先于消费方分布），
//! 消费方经 `merchants` 域路径逐类型显式 import。
//! 商户是参考数据字典（与分类/账户同款模式）：`name` 在用行全库唯一、
//! 软删除；交易以 `merchant_id` 引用（见核心交易域 Transaction），
//! 改名/软删经商户域收敛（见 `crate::merchants`）。
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

/// 商户关联交易计数行（issue #445，毛笔数口径）：商户字典行 → 引用它的未删流水
/// 毛笔数。实时按流水推导、不落库（读模型，无持久化状态）；软删商户照常计数、
/// 无引用商户计 0。仅供商户管理列表消费，不影响既有消费方。
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
