//! 实物资产领域模型（issue #466 / spec #465 / ADR-0064）：资产实体、建档入参、
//! 列表返回（含在持估值合计）。
//!
//! 术语边界见实物资产分域词汇表（`docs/contexts/CONTEXT-physical-asset.md`）
//! 与 ADR-0064：实物资产是大件实物的估值档案（单列小域，先例物品/保单），
//! 与物品域按「要不要跟踪市值」互斥分家。估值全手动、只追加不改写，
//! 当前估值 = 最新一条估值历史行；金额一律整数分；当前估值折本位币走
//! Amount 接缝（当期汇率，缺汇率错误上抛）。消费方经本域路径逐类型显式 import。

use serde::{Deserialize, Serialize};

use crate::db::query::FromRow;

/// 实物资产生命周期状态：`holding`（在持，估值进在持合计）/ `disposed`
/// （已处置，退出默认列表与合计，档案保留可回看）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalAssetStatus {
    Holding,
    Disposed,
}

impl PhysicalAssetStatus {
    /// 数据库存储的状态字符串（与 serde 序列化同形）。
    pub const fn as_str(self) -> &'static str {
        match self {
            PhysicalAssetStatus::Holding => "holding",
            PhysicalAssetStatus::Disposed => "disposed",
        }
    }

    /// 从状态字符串解析；未知值报参数错误。
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

// rusqlite：从 `physical_assets.status` 列直接读为枚举（DB 边界：TEXT 列经
// [`PhysicalAssetStatus::parse`] 严格映射，未知值即 FromSql 错误——DB CHECK
// 约束（V015）保证正常数据不可达）。先例：物品域 ItemStatus。
impl rusqlite::types::FromSql for PhysicalAssetStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        PhysicalAssetStatus::parse(value.as_str()?)
            .map_err(|e| rusqlite::types::FromSqlError::Other(e.into()))
    }
}

/// 数据库行记录（资产全列 + 当前估值三件套，JOIN 最新估值历史行）：
/// 列表 / 详情共用的读中间结构；折本位币经 Amount 接缝计算后转
/// [`PhysicalAsset`]（`FromRow` 只有行访问拿不到连接，折算在域函数内做）。
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

/// 实物资产实体（读模型）：资产档案全字段 + 当前估值（最新一条估值历史行，
/// 建档必填估值保证恒存在）+ 当前估值折本位币。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalAsset {
    pub id: String,
    /// 资产名称（建档必填）。
    pub name: String,
    /// 购买日期（可空；YYYY-MM-DD）。
    pub purchase_date: Option<String>,
    /// 购买价（可空，整数分；纯记录，不进任何金额口径）。
    pub purchase_price_cents: Option<i64>,
    /// 购买价币种（与购买价成对：购买价存在时必填）。
    pub purchase_currency_code: Option<String>,
    /// 生命周期状态（在持/已处置）。
    pub status: PhysicalAssetStatus,
    /// 处置日期（仅 disposed；YYYY-MM-DD；处置必填）。
    pub disposal_date: Option<String>,
    /// 处置价（可空，整数分；纯记录）。
    pub disposal_price_cents: Option<i64>,
    /// 处置价币种（与处置价成对）。
    pub disposal_currency_code: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
    /// 当前估值（整数分）= 最新一条估值历史行金额。
    pub current_valuation_cents: i64,
    /// 当前估值币种。
    pub current_valuation_currency_code: String,
    /// 当前估值日期（YYYY-MM-DD）——「这个数是多久前估的」的展示依据。
    pub current_valuation_date: String,
    /// 当前估值折本位币（整数分，Amount 接缝当期汇率）：仅**在持**行有值
    /// （在持估值进合计），已处置行不折算为 `None`。缺汇率时列表/详情整体
    /// 报错上抛，不以零或缺项静默通过（域口径，先例物品合计）。
    pub current_valuation_native_cents: Option<i64>,
    /// 本位币币种代码（全局默认币种，折算基准）。
    pub native_currency: String,
}

/// 建档入参（issue #466 T1）：名称必填、当前估值必填（即第一条估值历史行）、
/// 购买信息可选。编辑（名称/购买信息）与更新估值由 T2 承接，不共用本入参。
///
/// 校验归 `physical_asset` 域：名称 trim 非空；估值必填且 > 0、币种必填且存在；
/// 估值日期可空（= 今天）、可解析、拒绝未来（估值是已发生的判断）；购买价与
/// 币种成对（购买价存在时币种必填且存在，购买价缺省时币种忽略存空）。
#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetInput {
    pub name: String,
    /// 购买日期（可空；YYYY-MM-DD）。
    pub purchase_date: Option<String>,
    /// 购买价（可空，整数分）。
    pub purchase_price_cents: Option<i64>,
    /// 购买价币种（购买价存在时必填）。
    pub purchase_currency_code: Option<String>,
    /// 当前估值（整数分；必填——缺失显式报错而非默认 0）。
    pub initial_valuation_cents: Option<i64>,
    /// 当前估值币种（必填；前端预选默认币种）。
    pub initial_valuation_currency_code: Option<String>,
    /// 当前估值日期（可空 = 今天；YYYY-MM-DD）。
    pub initial_valuation_date: Option<String>,
}

/// 编辑档案入参（issue #467 T2）：仅名称与购买信息——估值不出现在编辑表单，
/// 只能经「更新估值」变更（估值历史只追加不改写，ADR-0064）。
///
/// 校验语义与建档同源：名称 trim 非空；购买价与币种成对（购买价存在时币种
/// 必填且存在，购买价缺省时币种忽略存空）。无估值字段，缺失即结构性排除。
#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetUpdateInput {
    pub name: String,
    /// 购买日期（可空；YYYY-MM-DD）。
    pub purchase_date: Option<String>,
    /// 购买价（可空，整数分）。
    pub purchase_price_cents: Option<i64>,
    /// 购买价币种（购买价存在时必填）。
    pub purchase_currency_code: Option<String>,
}

/// 更新估值入参（issue #467 T2）：每次调用追加一条估值历史行（旧值保留
/// 不覆盖），当前估值变为最新一条（估值日期最新，同日按插入序）。
///
/// 校验语义与建档首条估值同源：金额必填且 > 0、币种必填且存在；估值日期
/// 可空（= 今天）、可解析、拒绝未来（估值是已发生的判断）。
#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetValuationInput {
    /// 估值金额（整数分；必填——缺失显式报错而非默认 0）。
    pub amount_cents: Option<i64>,
    /// 估值币种（必填；前端预选当前估值币种）。
    pub currency_code: Option<String>,
    /// 估值日期（可空 = 今天；YYYY-MM-DD；可补过去，拒绝未来）。
    pub valuation_date: Option<String>,
}

/// 处置入参（issue #468 T3）：处置日期必填、处置价 + 币种可选纯记录。
///
/// 校验归 `physical_asset` 域：处置日期必填显式报错（错误码化）、可解析、
/// 拒绝未来（已发生的判断）、不早于购买日期（有购买日期时）；处置价与币种
/// 成对（处置价存在时币种必填且须存在，处置价缺省时币种忽略存空，先例购买价）。
#[derive(Debug, Clone, Deserialize)]
pub struct PhysicalAssetDisposeInput {
    /// 处置日期（必填；YYYY-MM-DD）。
    pub disposal_date: Option<String>,
    /// 处置价（可空，整数分；纯记录，不进任何金额口径）。
    pub disposal_price_cents: Option<i64>,
    /// 处置价币种（处置价存在时必填）。
    pub disposal_currency_code: Option<String>,
}

/// 列表返回：资产行（按筛选状态）+ **在持**估值合计（口径与筛选无关——
/// 「家底合计」恒指在持资产，回看已处置时合计不变）。
#[derive(Debug, Clone, Serialize)]
pub struct PhysicalAssetList {
    pub assets: Vec<PhysicalAsset>,
    /// 在持资产当前估值折本位币合计（整数分）。
    pub holding_total_native_cents: i64,
    /// 本位币币种代码（合计的折算基准）。
    pub native_currency: String,
}
