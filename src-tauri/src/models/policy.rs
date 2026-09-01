//! 保单领域模型：保单实体与建档入参（issue #360 / spec #358 / ADR-0051）。
//!
//! 术语边界见保险分域词汇表 `Policy` 条目与 ADR-0051：保单是消费型保险合同的
//! **静态档案**，与物品域 Item 同为独立领域概念——不是参考数据字典行，不是
//! 交易流水，也不是生成流水的协议。保司复用商户（Merchant）字典；保障期间
//! 止日可空（= 长期/终身）；保额可选、纯展示、不进任何金额口径；删除为软删除。

use serde::{Deserialize, Serialize};

use crate::db::query::FromRow;

/// 保单实体（读模型，全字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    /// 保险公司（商户字典引用，ADR-0028/0051）：不建第二份保司字典。
    pub merchant_id: String,
    /// 保单号。
    pub policy_number: String,
    /// 险种名称。
    pub product_name: String,
    /// 保障期间起（YYYY-MM-DD）。
    pub start_date: String,
    /// 保障期间止（YYYY-MM-DD）；`None` = 长期/终身（到期由期间推导，不持久化状态）。
    pub end_date: Option<String>,
    /// 保额（整数分，可选）：纯展示，不进任何金额口径。
    pub coverage_amount_cents: Option<i64>,
    /// 保额币种（与保额成对：保额存在时必填）。
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
            merchant_id: row.get("merchant_id")?,
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

/// 创建/编辑保单共用入参（issue #360）：静态合同要素全量替换（同物品编辑语义）。
///
/// 校验归 `commands::policy`：保司必须为在用商户（软删商户不可再被新档案选择）、
/// 保单号/险种非空、日期可解析且止日不早于起日、保额与币种成对（保额存在时
/// 币种必填且须存在；保额缺省时币种忽略存空）。
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyInput {
    pub merchant_id: String,
    pub policy_number: String,
    pub product_name: String,
    /// 保障期间起（YYYY-MM-DD）。
    pub start_date: String,
    /// 保障期间止（YYYY-MM-DD，可空 = 长期/终身）。
    pub end_date: Option<String>,
    /// 保额（整数分，可选；纯展示，不进任何金额口径）。
    pub coverage_amount_cents: Option<i64>,
    /// 保额币种（保额存在时必填）。
    pub coverage_currency_code: Option<String>,
    /// 备注（可选）。
    pub note: Option<String>,
}
