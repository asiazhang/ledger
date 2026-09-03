//! 分类领域模型（#419 随域归位）：分类实体与入参、排序项。
//!
//! 自全局模型目录迁入本域（#417 归属原则：实体归属优先于消费方分布），
//! 消费方经 `categories` 域路径逐类型显式 import。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CategoryInput {
    pub name: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CategoryUpdateInput {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub parent_id: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ReorderItem {
    pub id: String,
    pub sort_order: i64,
}

impl FromRow for Category {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Category {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            parent_id: row.get(3)?,
            icon: row.get(4)?,
            sort_order: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            version: row.get(8)?,
            device_id: row.get(9)?,
            is_deleted: row.get::<_, i64>(10)? != 0,
        })
    }
}
