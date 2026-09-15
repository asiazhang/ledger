//! 分类领域模型（#419 随域归位）：分类实体与入参、排序项。
//!
//! 自全局模型目录迁入本域（#417 归属原则：实体归属优先于消费方分布），
//! 消费方经 `categories` 域路径逐类型显式 import。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use ledger_infra::db::query::FromRow;

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
    /// 三态语义：**键缺席 = 不改、`null` = 清空图标、给值 = 落定该图标**。
    ///
    /// 与 [`Self::parent_id`] 同一根因、同一区分器：编辑弹窗的图标输入框清空后送
    /// `null`（表达「不要图标了」），单层 `Option<String>` 只能把 `null` 读成
    /// 「不改」——「清空图标」在 wire 上于是不可达（issue #1327 范围外修复）。
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub icon: Option<Option<String>>,
    /// 三态语义：**键缺席 = 不改、`null` = 清空（提升为顶级分类）、给值 = 落定该父**。
    ///
    /// 区分器 [`ledger_infra::serde_util::double_option`]（serde 默认把「键缺席」
    /// 与「值为 `null`」折叠成同一 `None`，原理见其模块文档）：不分开则「无父分类」
    /// 在 wire 上不可达——编辑弹窗选「无父分类」会静默不生效（修复留痕见 #1327
    /// 交付报告）。
    #[serde(default, deserialize_with = "ledger_infra::serde_util::double_option")]
    pub parent_id: Option<Option<String>>,
}

/// 排序重排项（IPC 入参；同步命令 Reorder 变体随行载荷复用，issue #860）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
