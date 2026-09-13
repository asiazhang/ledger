//! 交易实体与读模型（共享语义区，issue #423）：交易行、来源/转换投影与分页结果。
//!
//! 职责：交易全量读模型类型的单一定义点。不变量：`source` / `convert` 非库列，
//! `FromRow` 恒填空、由读路径 `attach_*` 按页填充；类型经 `crate::model` 逐项再导出。
//! ADR 指针：ADR-0113 决策 2/8。陷阱：列序与 `transactions` 表 SELECT 顺序一一对应。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::amount::TransactionKind;
use ledger_infra::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Transaction {
    pub id: String,
    /// 交易类型枚举（serde 小写字符串序列化，wire 与裸 String 一致）。
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: String,
    pub to_account_id: Option<String>,
    /// 可选出资账户（issue #935 / ADR-0096）：仅 buy/sell 可携带（行为层准入收口
    /// `funding::validate_funding_account`），结算现金腿的归因端点。
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    /// 可选保单引用（issue #361 / ADR-0051 决策 3）：仅 expense/income 可挂（行为层准入）。
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
    /// 来源列（spec #704 / issue #706，词汇表「来源列」）：发起来源实体的读时反查推导，
    /// 零数据迁移。仅列表/搜索读路径填充（`attach_sources`）；单笔读回与写入响应
    /// 不做反查，恒为 `None`；无来源交易（手动录入/AI 导入）为 `None`。
    pub source: Option<TransactionSource>,
    /// 转换两腿扩展（仅 convert，ADR-0099 / 词汇表「基金转换（Conversion）」）：
    /// 列表/搜索读路径填充，非转换行恒 `None`。
    /// 列表金额列展示转出金额（`out_amount_cents`），不读行金额锚点（锚点是结转成本）。
    pub convert: Option<ConvertFields>,
}

/// 基金转换扩展（ADR-0099）：一笔 convert 两腿的标的、份额与两侧确认金额。
///
/// 行金额锚点（`transactions.amount_cents`）= 服务端按 FIFO 消耗算出的**结转成本**，
/// 不是确认单金额；两侧确认金额存本扩展（展示与多腿分摊口径的输入）。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ConvertFields {
    /// 转入标的。
    pub to_instrument_id: String,
    /// 转入标的代码。
    pub to_symbol: String,
    /// 转入份额。
    pub to_quantity: f64,
    /// 转出金额（分，确认单权威）。
    pub out_amount_cents: i64,
    /// 转入金额（分，确认单权威）。
    pub in_amount_cents: i64,
}

/// 交易来源类型闭集（spec #704 / issue #706，词汇表「来源列」）：定时计划三形态 /
/// 保单 / 物品 / 标的。wire 为 camelCase 字符串（与前端词表同字面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransactionSourceKind {
    /// 分期计划（期次对生成交易反查，#707 接线）。
    InstallmentPlan,
    /// 订阅计划（期次对生成交易反查，#707 接线）。
    Subscription,
    /// 定时转账计划（期次对生成交易反查，#707 接线）。
    ScheduledTransfer,
    /// 保单（PolicyReference 直挂，本票接线）。
    Policy,
    /// 物品（溯源指针反查，本票接线）。
    Item,
    /// 投资标的（证券交易记录反查，#709 接线）。
    Instrument,
}

/// 来源状态闭集（spec #704，可空字段）：`None` = 无状态标注。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum TransactionSourceStatus {
    /// 已取消（计划，#707 接线）。
    Cancelled,
    /// 已处置（物品，本票接线）。
    Disposed,
    /// 已删除（软删保单，本票接线；历史引用保留不置空，ADR-0051 决策 5）。
    Deleted,
}

/// 交易行来源（spec #704 / issue #706）：展示名 + 可空状态，随行返回供来源列渲染。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TransactionSource {
    /// 来源类型（六类闭集）。
    pub kind: TransactionSourceKind,
    /// 来源实体 id。
    pub entity_id: String,
    /// 展示名（保单 = 险种名；其余口径见各消费票）。
    pub display_name: String,
    /// 来源状态（可空）：软删保单 = deleted。
    pub status: Option<TransactionSourceStatus>,
}

/// 交易搜索分页结果（服务端分页）。
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct TransactionSearchResult {
    /// 匹配交易（当前页）。
    pub items: Vec<Transaction>,
    /// 命中总数（供「命中 N 条」与分页）。
    pub total: i64,
}

/// 备注拼音回填失败阶段（issue #513）：报告内可本地化的失败位置——
/// 积压探测 / 读取积压行 / 开启批事务 / 写入批内行 / 提交批事务。
#[derive(Debug, Serialize, ToSchema)]
pub struct TransactionListResult {
    pub items: Vec<Transaction>,
    /// 满足过滤条件的未删除交易总数（用于分页条）。
    pub total: i64,
}

impl FromRow for Transaction {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Transaction {
            id: row.get(0)?,
            kind: row.get(1)?,
            amount_cents: row.get(2)?,
            currency_code: row.get(3)?,
            amount_native_cents: row.get(4)?,
            account_id: row.get(5)?,
            to_account_id: row.get(6)?,
            funding_account_id: row.get(7)?,
            category_id: row.get(8)?,
            refund_of_transaction_id: row.get(9)?,
            note: row.get(10)?,
            date: row.get(11)?,
            created_at: row.get(12)?,
            updated_at: row.get(13)?,
            version: row.get(14)?,
            device_id: row.get(15)?,
            is_deleted: row.get::<_, i64>(16)? != 0,
            merchant_id: row.get(17)?,
            policy_id: row.get(18)?,
            // 来源列非库列：FromRow 恒空，由列表/搜索读路径 `attach_sources` 按页填充。
            source: None,
            // 转换扩展同规：非库列，由列表/搜索读路径 `attach_convert_fields` 按页填充。
            convert: None,
        })
    }
}
