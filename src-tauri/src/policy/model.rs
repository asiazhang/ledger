//! 保单领域模型（#420 随域归位）：保单实体、建档入参与保单视角统计行
//! （issue #360 / spec #358 / ADR-0051）。
//!
//! 自全局模型目录迁入本域（#417 归属原则），消费方经 `policy` 域路径
//! 逐类型显式 import。术语边界见保险分域词汇表 `Policy` 条目与 ADR-0051：保单是消费型保险合同的
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
/// 校验归 `policy` 域：保司必须为在用商户（软删商户不可再被新档案选择）、
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

/// 来源列展示反查投影（spec #704 / issue #706）：按 id 批量取保单展示字段的最小行——
/// 险种名（来源列展示名）+ 软删标志（来源状态），供核心交易域按页填充来源列。
#[derive(Debug, Clone)]
pub struct PolicySourceDisplay {
    pub id: String,
    /// 险种名称（来源列展示名）。
    pub product_name: String,
    /// 软删标志：历史引用保留不置空（ADR-0051 决策 5），软删保单照常返回。
    pub is_deleted: bool,
}

impl FromRow for PolicySourceDisplay {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(PolicySourceDisplay {
            id: row.get(0)?,
            product_name: row.get(1)?,
            is_deleted: row.get::<_, i64>(2)? != 0,
        })
    }
}

/// 逐保单视角统计行（issue #363 / ADR-0051 决策 5/6）：全部字段实时推导，
/// **不落库、不摊销**（与 SubscriptionSpend 实际花费口径同纪律）——
/// 每个数字可逐笔对账到挂单流水。
#[derive(Debug, Clone, Serialize)]
pub struct PolicyStats {
    /// 保单 id（与 [`Policy::id`] 对应；只含未删除保单）。
    pub policy_id: String,
    /// 折算基准币种（`default_currency_code`，全局默认币种）：下列两个合计
    /// 均为流水的 `amount_native_cents` 忠实合计（读取期不二次折算）。
    pub native_currency: String,
    /// 累计已缴保费（本位币，分）：挂单保费（`expense`）流水合计。
    pub total_paid_native_cents: i64,
    /// 累计现金流入（本位币，分）：挂单现金流入（`income`）流水合计
    /// （理赔/退保/满期返还，ADR-0051 决策 4）。
    pub total_inflow_native_cents: i64,
    /// 下期扣款日（YYYY-MM-DD）：该保单**活跃**缴费协议（订阅形态，含多段历史中
    /// 的 active 段）的最早 pending 期次；无活跃协议或无 pending 期次 = `None`
    /// （界面不显示该字段，可推导的状态不落库）。
    pub next_charge_date: Option<String>,
    /// 到期态（实时推导，不持久化，ADR-0051 决策 5）：保障期间止日非空且早于
    /// today → 已到期；止日为空 = 长期/终身 → 恒 `false`。
    pub is_expired: bool,
}
