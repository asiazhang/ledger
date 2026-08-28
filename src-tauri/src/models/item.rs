//! 物品领域模型：物品实体、创建入参与列表项（含每天使用成本）。
//!
//! 术语边界见 CONTEXT.md `Item` / `DailyUsageCost` 条目与 ADR-0014：
//! 物品是独立领域实体，与投资标的（Instrument）严格区分；金额沿用
//! 整数分 + raw/native 分离约定，折算走 Amount 接缝。

use serde::{Deserialize, Serialize};

use crate::db::query::FromRow;

/// 物品生命周期状态：`in_use`（在用，摊到今天）/ `disposed`（已处置，摊到处置日）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    InUse,
    Disposed,
}

impl ItemStatus {
    /// 数据库存储的状态字符串（与 serde 序列化同形）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ItemStatus::InUse => "in_use",
            ItemStatus::Disposed => "disposed",
        }
    }

    /// 从状态字符串解析；未知值报参数错误。
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

// rusqlite：从 `items.status` 列直接读为枚举（DB 边界：TEXT 列经 [`ItemStatus::parse`]
// 严格映射，未知值即 FromSql 错误——DB CHECK 约束（V009）保证正常数据不可达）。
impl rusqlite::types::FromSql for ItemStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        ItemStatus::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

/// 物品实体（读模型，全字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    /// 购买日期（YYYY-MM-DD）。
    pub purchase_date: String,
    /// 总成本（原始币种，整数分）。
    pub total_cost_cents: i64,
    pub currency_code: String,
    /// 总成本折算本位币（默认币种，整数分，Amount 接缝折算）。
    pub cost_native_cents: i64,
    /// 生命周期状态（serde 小写字符串序列化）。
    pub status: ItemStatus,
    /// 处置日期（仅 disposed；YYYY-MM-DD）。
    pub disposal_date: Option<String>,
    /// 残值（仅 disposed 可填，整数分）。
    pub residual_value_cents: Option<i64>,
    /// 关联购买交易 id（溯源，可空）：仅为自动带出日期/成本与溯源，
    /// 不建立「交易→物品」反向引用（CONTEXT.md Item 条目）。
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

/// 创建物品入参（issue #115 骨架：创建即「在用」物品）。
///
/// 生命周期流转（标记已处置/残值）由后续票（#120）扩展；关联购买交易
/// 自动带出由 #119 扩展。金额必须 > 0、名称非空、日期可解析。
#[derive(Debug, Clone, Deserialize)]
pub struct ItemInput {
    pub name: String,
    /// 购买日期（YYYY-MM-DD）。
    pub purchase_date: String,
    /// 总成本（原始币种，整数分），必须 > 0。
    pub total_cost_cents: i64,
    /// 原始币种代码（须可按 Amount 接缝折算到默认币种）。
    pub currency_code: String,
    /// 备注（可选）。
    pub note: Option<String>,
    /// 可选关联的购买交易 id：创建时提供（或修改时提供与既有不同的 id）→
    /// 校验交易存在且为 expense，并自动带出购买日期与基础成本（覆盖同名字段）；
    /// 修改时提供与既有相同的 id 或省略（None）→ 维持现状，不重新带出。
    pub purchase_transaction_id: Option<String>,
}

/// 处置物品入参（issue #120）：生命周期流转 `in_use` → `disposed`。
/// 处置日期必填（不得早于购买日期，否则每天成本口径不可达）；残值可选，
/// `Some` 时必须 ≥ 0。残值 ≥ 成本合法（分子下限 0，`item::cost` 口径）。
#[derive(Debug, Clone, Deserialize)]
pub struct ItemDisposeInput {
    /// 处置日期（YYYY-MM-DD），必填；每天成本分母摊到该日。
    pub disposal_date: String,
    /// 残值（整数分，可选）：`None` 视同无残值（分子 = 总成本）。
    pub residual_value_cents: Option<i64>,
}

/// 物品列表项：物品实体 + 每天使用成本快照（经 `item::cost` 接缝计算，
/// 调用方不另写口径）。`used_days` / `per_day_cents` 为查询时刻快照，不落库。
/// 成本分解三元组（分子 `numerator_cents` ÷ 天数 `used_days` = `per_day_cents`）
/// 一并返回，供物品详情视图直接展示，避免调用方反推口径。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemWithDailyCost {
    #[serde(flatten)]
    pub item: Item,
    /// 已用天数：购买日 → 目标日的日历天数，含起止两端（在用 = 今天，已处置 = 处置日）。
    pub used_days: i64,
    /// 成本分解分子（分）：总成本 − 残值，下限 0（在用未填残值时即总成本）。
    pub numerator_cents: i64,
    /// 每天成本（分/天，**小数**）：`item::cost` 接缝计算，仅供展示。
    pub per_day_cents: f64,
}

/// 每天使用成本计算结果（issue #121 自选参考日重算）：不带物品实体的独立形态，
/// 供单件计算命令（`calculate_item_cost`）返回；三元组与 [`ItemWithDailyCost`]
/// 同一口径（均经 `item::cost` 接缝），前端详情视图重算展示共用。
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ItemDailyCost {
    /// 已用天数：购买日 → 目标日（参考日或缺省目标日）的日历天数，含起止两端。
    pub used_days: i64,
    /// 成本分解分子（分）：总成本 − 残值，下限 0。
    pub numerator_cents: i64,
    /// 每天成本（分/天，**小数**），仅供展示。
    pub per_day_cents: f64,
}
