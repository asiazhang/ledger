use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::amount::{FxRateSource, TransactionKind};
use ledger_infra::db::query::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Transaction {
    pub id: String,
    #[doc = "交易类型枚举（serde 小写字符串序列化，wire 与裸 String 一致）。"]
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    #[doc = "出资账户引用（ADR-0138）：分解行（携出资子行的 expense/income、显式覆盖的 refund、继承分解原支出的缺省 refund）读回 `None`——AI / HTTP 契约类型域 `string → string | null`（已发布契约放宽，CHANGELOG BREAKING）；单出资行读回形态与现状兼容。"]
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    #[doc = "可选出资账户（issue #935 / ADR-0096）：仅 buy/sell 可携带（行为层准入收口 `funding::validate_funding_account`），结算现金腿的归因端点。"]
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    #[doc = "可选保单引用（issue #361 / ADR-0051 决策 3）：仅 expense/income 可挂（行为层准入）。"]
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    #[doc = "折算来源留痕（issue #1548 / ADR-0011 修订，V029）：非本位币行记录本笔折算使用的汇率值与来源（`native ≈ amount_cents × fx_rate_used`，到分）；与本位币同币种、无现金腿（split）及写入时点早于留痕功能的存量行为 `None`。读回解释用，不参与检索与幂等身份。"]
    pub fx_rate_used: Option<f64>,
    pub fx_rate_source: Option<FxRateSource>,
    pub note: Option<String>,
    #[doc = "来源订单号（V035）：导入来源单据的外部订单标识（详情订单区的归属锚点，#1928）；来源元数据不限 kind，手动行为空；单号不入备注（备注可编辑、搜索按原文命中）。"]
    pub source_order_no: Option<String>,
    pub date: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
    #[doc = "出资项分解（issue #1860 / ADR-0138）：分解行按顺序位稳定返回；非库列——`FromRow` 恒空，由读路径 `attach_fundings` 填充（列表与单笔读回都填）。refund 缺省派生的条目（读时按原支出比例推导、不落库）带 `derived=true`。"]
    #[serde(default)]
    pub fundings: Vec<TransactionFunding>,
    #[doc = "购买项明细（issue #1882 / ADR-0138 决策 9）：带购买项的 expense 按对账单顺序稳定返回；非库列——`FromRow` 恒空，由读路径 `attach_purchases` 填充（列表 / 详情 / 搜索 / 订单汇总同填）。仅 `expense` 可带；无购买项恒空数组。"]
    #[serde(default)]
    pub purchases: Vec<TransactionPurchase>,
    #[doc = "来源列（spec #704 / issue #706，词汇表「来源列」）：发起来源实体的读时反查推导，零数据迁移。仅列表/搜索读路径填充（`attach_sources`）；单笔读回与写入响应不做反查，恒为 `None`；无来源交易（手动录入/AI 导入）为 `None`。"]
    pub source: Option<TransactionSource>,
    #[doc = "转换两腿扩展（仅 convert，ADR-0099 / 词汇表「基金转换（Conversion）」）：列表/搜索读路径填充，非转换行恒 `None`。列表金额列展示转出金额（`out_amount_cents`），不读行金额锚点（锚点是结转成本）。"]
    pub convert: Option<ConvertFields>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "出资项读回条目（issue #1860 / ADR-0138）：分解行随交易读回，按顺序位稳定排序。"
)]
pub struct TransactionFunding {
    #[doc = "出资账户 id。"]
    pub account_id: String,
    #[doc = "出资金额（正整数分）：Σ 全部条目 == 交易金额。"]
    pub amount_cents: i64,
    #[doc = "扣款标签（可选自由文本）。"]
    pub label: Option<String>,
    #[doc = "refund 缺省按出资比例派生的读时推导标记（ADR-0138 决策 5）：`true` = 读时推导、未落库；落库分解与单出资行恒 `false`。"]
    pub derived: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "购买项读回条目（issue #1882 / ADR-0138 决策 9/10）：带购买项的 expense 随交易读回，按顺序位（对账单顺序）稳定排序。"
)]
pub struct TransactionPurchase {
    #[doc = "商品名称（源单原文）。"]
    pub name: String,
    #[doc = "件数（正整数）。"]
    pub quantity: i64,
    #[doc = "商品分类引用（可选，明细指针）：分类删除时读回 null；不承载统计口径（报表按交易行分类整单计入，ADR-0138 决策 11）。"]
    pub category_id: Option<String>,
    #[doc = "商品单价（可选，整数分）：源单只给订单总额时为空；存而不显示、不进任何金额与折算口径（ADR-0138 决策 10）。"]
    pub unit_price_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "基金转换扩展（ADR-0099）：一笔 convert 两腿的标的、份额与两侧确认金额。\n\n行金额锚点（`transactions.amount_cents`）= 服务端按 FIFO 消耗算出的**结转成本**，不是确认单金额；两侧确认金额存本扩展（展示与多腿分摊口径的输入）。"
)]
pub struct ConvertFields {
    #[doc = "转入标的。"]
    pub to_instrument_id: String,
    #[doc = "转入标的代码。"]
    pub to_symbol: String,
    #[doc = "转入份额。"]
    pub to_quantity: f64,
    #[doc = "转出金额（分，确认单权威）。"]
    pub out_amount_cents: i64,
    #[doc = "转入金额（分，确认单权威）。"]
    pub in_amount_cents: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "交易来源类型闭集（spec #704 / issue #706，词汇表「来源列」）：定时计划三形态 / 保单 / 物品 / 标的。wire 为 camelCase 字符串（与前端词表同字面）。"
)]
#[serde(rename_all = "camelCase")]
pub enum TransactionSourceKind {
    #[doc = "分期计划（期次对生成交易反查，#707 接线）。"]
    InstallmentPlan,
    #[doc = "订阅计划（期次对生成交易反查，#707 接线）。"]
    Subscription,
    #[doc = "定时转账计划（期次对生成交易反查，#707 接线）。"]
    ScheduledTransfer,
    #[doc = "保单（PolicyReference 直挂，本票接线）。"]
    Policy,
    #[doc = "物品（溯源指针反查，本票接线）。"]
    Item,
    #[doc = "投资标的（证券交易记录反查，#709 接线）。"]
    Instrument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[schema(description = "来源状态闭集（spec #704，可空字段）：`None` = 无状态标注。")]
#[serde(rename_all = "camelCase")]
pub enum TransactionSourceStatus {
    #[doc = "已取消（计划，#707 接线）。"]
    Cancelled,
    #[doc = "已处置（物品，本票接线）。"]
    Disposed,
    #[doc = "已删除（软删保单，本票接线；历史引用保留不置空，ADR-0051 决策 5）。"]
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "交易行来源（spec #704 / issue #706）：展示名 + 可空状态，随行返回供来源列渲染。"
)]
pub struct TransactionSource {
    #[doc = "来源类型（六类闭集）。"]
    pub kind: TransactionSourceKind,
    #[doc = "来源实体 id。"]
    pub entity_id: String,
    #[doc = "展示名（保单 = 险种名；其余口径见各消费票）。"]
    pub display_name: String,
    #[doc = "来源状态（可空）：软删保单 = deleted。"]
    pub status: Option<TransactionSourceStatus>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
#[schema(description = "交易搜索分页结果（服务端分页）。")]
pub struct TransactionSearchResult {
    #[doc = "匹配交易（当前页）。"]
    pub items: Vec<Transaction>,
    #[doc = "命中总数（供「命中 N 条」与分页）。"]
    pub total: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TransactionListResult {
    pub items: Vec<Transaction>,
    #[doc = "满足过滤条件的未删除交易总数（用于分页条）。"]
    pub total: i64,
}

pub(crate) const ROW_COLUMNS: &[&str] = &[
    "id",
    "kind",
    "amount_cents",
    "currency_code",
    "amount_native_cents",
    "account_id",
    "to_account_id",
    "funding_account_id",
    "category_id",
    "refund_of_transaction_id",
    "note",
    "date",
    "created_at",
    "updated_at",
    "version",
    "device_id",
    "is_deleted",
    "merchant_id",
    "policy_id",
    "fx_rate_used",
    "fx_rate_source",
    "source_order_no",
];

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
            fx_rate_used: row.get(19)?,
            fx_rate_source: row.get(20)?,
            source_order_no: row.get(21)?,
            fundings: Vec::new(),
            purchases: Vec::new(),
            source: None,
            convert: None,
        })
    }
}
