use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::amount::TransactionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "证券扩展行的来源口径（issue #1343 / ADR-0115 修订）：投资域词汇「期初存量」的契约投影。\n\n- `trade`：真实成交（默认，也是缺省值）；\n- `opening`：期初存量——补记（持仓初始化）落进来的存量持仓，真实建仓时点未知。它不是当日真实入金，资金加权收益率据此把该标的判为「年化不适用」，改给未年化收益率。\n\n仅 buy 可携带（行为层准入守卫拒绝其余 kind）；wire 形态为小写字符串，与 `security_transactions.origin` 的取值一一对应。"
)]
#[serde(rename_all = "snake_case")]
pub enum SecurityOrigin {
    #[default]
    Trade,
    Opening,
}

impl SecurityOrigin {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Trade => "trade",
            Self::Opening => "opening",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "出资项分解条目（issue #1860 / ADR-0138）：`TransactionInput.funding` / `UpdateTransactionInput.funding` 的条目类型，也是同步命令归一化行随行的分解载荷（`NormalizedTransaction.funding`）——同一契约三处消费，只增不改。wire 形态：`{account_id, amount_cents, label?}`。"
)]
pub struct TransactionFundingInput {
    #[doc = "出资账户 id：准入与 `account_id` 同口径（存在、未软删、币种与交易一致、不折算），无账户类型闭集（credit / debt 可出资，白条场景）。允许同一账户多条出资项（预售定金 + 尾款靠标签区分）。"]
    pub account_id: String,
    #[doc = "出资金额（正整数分）：Σ 全部条目 == 交易金额。"]
    pub amount_cents: i64,
    #[doc = "扣款标签（可选自由文本，上限 50 字）：同账户多条出资项的区分标注。"]
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[schema(
    description = "购买项条目（issue #1882 / ADR-0138 决策 9/10）：`TransactionInput.purchases` / `UpdateTransactionInput.purchases` 的条目类型，也是同步命令归一化行随行的购买项载荷（`NormalizedTransaction.purchases`）——同一契约三处消费，只增不改。wire 形态：`{name, quantity, category_id?, unit_price_cents?}`；数组顺序即对账单顺序（用户可见语义），落库顺序位与读回顺序都按数组序。"
)]
pub struct TransactionPurchaseInput {
    #[doc = "商品名称（必填，源单原文）：购买项行自承载商品名，不写进交易备注。"]
    pub name: String,
    #[doc = "件数（正整数）。"]
    pub quantity: i64,
    #[doc = "商品分类引用（可选）：须为在用分类 id（AI 先经分类端点解析或即建）；引用已软删分类的历史行可编辑其他字段（保持历史引用）。"]
    pub category_id: Option<String>,
    #[doc = "商品单价（可选，整数分，≥ 0）：源单只给订单总额时**留空（不提交），不猜不编造**；价格存而不显示、不进任何金额与折算口径、不与实付勾稽。"]
    pub unit_price_cents: Option<i64>,
}
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TransactionInput {
    #[doc = "交易类型枚举（serde 小写字符串反序列化）。非法 kind 在反序列化阶段报 400（请求体格式错误，见 batch 端点描述）；合法值 wire 与裸 String 一致。"]
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    #[doc = "出资账户引用（ADR-0138 契约互斥）：与 `funding` 分解二选一——携带非空分解时必须缺省（码化报错），不带分解时必填。分解行的账户口径由子行承载，主表列落 NULL。"]
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    #[doc = "可选出资账户（issue #935 / ADR-0096）：仅 buy/sell 可携带，准入收口 `funding::validate_funding_account`；缺省即「结算账户 = 投资账户」。"]
    pub funding_account_id: Option<String>,
    #[doc = "出资分解（issue #1860 / ADR-0138）：每条 = {账户, 金额, 扣款标签}，全部条目之和恒等于交易金额；仅 expense / income 可携带（refund 显式覆盖同契约），与 `account_id` 互斥——非空分解时 `account_id` 必须缺省。缺省（空数组）即单出资现状，存量行为零变化。"]
    #[serde(default)]
    pub funding: Vec<TransactionFundingInput>,
    #[doc = "购买项明细（issue #1882 / ADR-0138 决策 9）：多商品订单的逐件商品行，仅 `expense` 可携带（其余 kind 携带即码化拒绝）；每条 = {名称, 件数, 分类?, 单价?}，数组顺序 = 对账单顺序。价格拿不到就留空（不猜不编造）；商品名写购买项，不再写进备注。缺省（空数组）即无明细，存量行为零变化。"]
    #[serde(default)]
    pub purchases: Vec<TransactionPurchaseInput>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    #[doc = "商户名字符串（AI 导入契约，issue #194 / ADR-0028）：提交体不带 `merchant_id` 而带商户名时，后端写入路径精确匹配在用商户名——命中复用、未命中即建，归一化责任收口在后端，AI 不负责商户去重。与 `merchant_id` 互斥（同时提供属请求错误）。"]
    pub merchant_name: Option<String>,
    #[doc = "可选保单引用（issue #361 / ADR-0051 决策 3）：仅 expense/income 可携带，其余 kind 行为层拒绝；引用不存在的保单返回 400（中文错误，可读回自纠）。"]
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    #[doc = "来源订单号（V035 / ADR-0138 决策 9）：导入来源单据的外部订单号，来源元数据、不限 kind；同单多行写同一单号、只写本字段不入备注；缺省（不提交）即不带。"]
    pub source_order_no: Option<String>,
    pub date: String,
    #[doc = "标的 id（仅 buy/sell/convert/split/dividend 需提供）：先用标的搜索端点（`GET /api/v1/instruments`）把源数据中的标的描述解析为 id，未命中再按创建端点幂等新建；引用不存在的标的返回 400（中文错误，可读回自纠）。dividend 不要求当前有持仓，到账账户为任意在用账户（ADR-0109）。"]
    pub instrument_id: Option<String>,
    #[doc = "成交数量（份，可含小数）：仅 buy/sell/convert/split 需提供。buy/sell 为成交份额、convert 为转出份额，均必须 > 0；split 为同一账户内单标的的带符号份额增量 Δ（`+` 折算/结转/送股；`−` 缩股且 |Δ| 小于当前持仓），必须 ≠ 0。split 无现金腿：`amount_cents` 恒填 0、`price_cents` 不提供、`fee_cents` 只能为 0，不携带 `to_instrument_id` / `to_account_id` / `funding_account_id` / 商户 / 分类 / 保单，落账前后全部账户余额不变，仅持仓份额与市值随 Δ 变化（ADR-0106 决策 1/7）。dividend 不提供数量（无份额变动）。"]
    pub quantity: Option<f64>,
    #[doc = "成交单价（万分之一元，元 × 10000；价格刻度见 ADR-0038，金额列仍为整数分）：非基金标的必填且必须 > 0；场外基金（issue #302 / ADR-0038 金额权威）不提供，由后端按（行金额 ∓ 手续费）÷ 数量反算到万分之一元；convert 不提供（由转出金额 ÷ 转出份额反算）。dividend 不提供（无成交单价）。"]
    pub price_cents: Option<i64>,
    #[doc = "手续费（整数分，可省，默认 0）：sell 不得超过卖出收入（数量 × 单价，非基金）；服务端行金额：非基金按 buy = 数量 × 单价 + 费用、sell = 数量 × 单价 − 费用重算；场外基金以 `amount_cents`（确认单整分金额）为权威，行金额原样采用；convert 如实记录、不进支出报表。dividend 不接受手续费（只能省或填 0）。"]
    pub fee_cents: Option<i64>,
    #[doc = "转入标的 id（仅 convert）：与 `instrument_id`（转出标的）必须不同。"]
    pub to_instrument_id: Option<String>,
    #[doc = "转入份额（仅 convert，必须 > 0）：转入批次建仓数量。"]
    pub to_quantity: Option<f64>,
    #[doc = "转出金额（仅 convert，整数分，必须 > 0）：确认单转出端金额（列表展示口径）。"]
    pub out_amount_cents: Option<i64>,
    #[doc = "转入金额（仅 convert，整数分，必须 > 0）：确认单转入端金额。"]
    pub in_amount_cents: Option<i64>,
    #[doc = "证券扩展行来源（issue #1343 / ADR-0115 修订）：仅 buy 可携带，`\"opening\"` = 期初存量——补记（持仓初始化）落进来的存量持仓，真实建仓时点未知。它不是当日真实入金，资金加权收益率据此不给该标的年化、改给未年化收益率。缺省（不提交）即 `\"trade\"` 真实成交；其余 kind 携带返回 400。"]
    pub origin: Option<SecurityOrigin>,
    #[doc = "可选逐笔显式汇率：折算乘数（本位币金额 = round(amount_cents × fx_rate)，到分），用于数据源覆盖不到的日期；优先级显式 > 序列 > 报错；缺省（不提交）即按交易日序列折算；非法值（非正、与本位币同币种携带）报码化错误、整笔不落库。"]
    pub fx_rate: Option<f64>,
    #[doc = "客户端提供的、内容无关的导入幂等键（指向\"该交易来自源文件哪一行\"）。带键时批量导入以其为准去重（同键跳过、内容无关）；无键时回退内容哈希兜底。"]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[schema(
    description = "交易修改请求体（`PUT /api/v1/transactions/{id}`）。\n\n与 `TransactionInput` 的唯一差异是不含 `idempotency_key`：幂等键不可编辑，只在导入时落定，编辑不改变导入身份（修改后重跑同批导入仍按同键去重、不产生重复）。buy/sell 仍需 `instrument_id`/`quantity`/`price_cents`/`fee_cents`；convert 的就地修改（全字段替换）同样需 `instrument_id`/`quantity`/`to_instrument_id`/`to_quantity`/`out_amount_cents`/`in_amount_cents`/`fee_cents`（kind 变更仍被拒，见 `behavior::update`）。"
)]
pub struct UpdateTransactionInput {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    #[doc = "出资账户引用（ADR-0138 全量语义）：`None` ⇔ 必须携非空 `funding` 分解（不用空串哨兵）；`Some` ⇔ 不得携分解。单 ⇄ 多就地互转走既有修改协议。"]
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    #[doc = "可选出资账户（与 `TransactionInput.funding_account_id` 同一契约）。"]
    pub funding_account_id: Option<String>,
    #[doc = "出资分解（与 `TransactionInput.funding` 同一契约，全量替换语义）：不携带（空数组）⇔ 必须携 `account_id`（分解被整体移除）；非空 ⇔ 禁 `account_id`。"]
    #[serde(default)]
    pub funding: Vec<TransactionFundingInput>,
    #[doc = "购买项明细（与 `TransactionInput.purchases` 同一契约，全量替换语义）：空（不携带）⇔ 无明细（整体移除）；仅 `expense` 可携带。"]
    #[serde(default)]
    pub purchases: Vec<TransactionPurchaseInput>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    #[doc = "商户名字符串（与 `TransactionInput.merchant_name` 同一契约）：修改路径同样由后端精确匹配复用或即建；解析出的 id 与该行当前商户相同即视为保持历史引用。"]
    pub merchant_name: Option<String>,
    #[doc = "可选保单引用（与 `TransactionInput.policy_id` 同一契约）：修改路径提交值与该行当前保单相同视为保持历史引用（已软删保单的历史交易仍可改其他字段）。"]
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    #[doc = "来源订单号（与 `TransactionInput.source_order_no` 同一契约）：全字段替换，缺省即清除。"]
    pub source_order_no: Option<String>,
    pub date: String,
    #[doc = "标的 id（仅 buy/sell/convert/split/dividend 需提供）：与 `TransactionInput` 同一契约；引用不存在的标的返回 400（中文错误，可读回自纠）。"]
    pub instrument_id: Option<String>,
    #[doc = "成交数量（份，可含小数）：与 `TransactionInput.quantity` 同一契约。"]
    pub quantity: Option<f64>,
    #[doc = "成交单价（万分之一元）：与 `TransactionInput.price_cents` 同一契约，必须 > 0。"]
    pub price_cents: Option<i64>,
    #[doc = "手续费（整数分，可省，默认 0）：与 `TransactionInput.fee_cents` 同一契约。"]
    pub fee_cents: Option<i64>,
    #[doc = "转入标的 id（仅 convert）：不得与转出标的相同。"]
    pub to_instrument_id: Option<String>,
    #[doc = "转入份额。"]
    pub to_quantity: Option<f64>,
    #[doc = "转出金额（分）。"]
    pub out_amount_cents: Option<i64>,
    #[doc = "转入金额（分）。"]
    pub in_amount_cents: Option<i64>,
    #[doc = "证券扩展行来源（与 `TransactionInput.origin` 同一契约）：仅 buy 可携带，`\"opening\"` = 期初存量。就地修改为全字段替换，缺省即回落 `\"trade\"`。"]
    pub origin: Option<SecurityOrigin>,
    #[doc = "可选逐笔显式汇率：同 `TransactionInput.fx_rate`，缺省回退序列查询。"]
    pub fx_rate: Option<f64>,
}

impl From<UpdateTransactionInput> for TransactionInput {
    fn from(u: UpdateTransactionInput) -> Self {
        TransactionInput {
            kind: u.kind,
            amount_cents: u.amount_cents,
            currency_code: u.currency_code,
            account_id: u.account_id,
            to_account_id: u.to_account_id,
            funding_account_id: u.funding_account_id,
            funding: u.funding,
            purchases: u.purchases,
            category_id: u.category_id,
            merchant_id: u.merchant_id,
            merchant_name: u.merchant_name,
            policy_id: u.policy_id,
            refund_of_transaction_id: u.refund_of_transaction_id,
            note: u.note,
            source_order_no: u.source_order_no,
            date: u.date,
            instrument_id: u.instrument_id,
            quantity: u.quantity,
            price_cents: u.price_cents,
            fee_cents: u.fee_cents,
            to_instrument_id: u.to_instrument_id,
            to_quantity: u.to_quantity,
            out_amount_cents: u.out_amount_cents,
            in_amount_cents: u.in_amount_cents,
            origin: u.origin,
            fx_rate: u.fx_rate,
            idempotency_key: None,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreateTransactionResult {
    pub success: bool,
    pub duplicate: bool,
    pub id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct TransactionBatchInput {
    pub transactions: Vec<TransactionInput>,
    #[serde(default = "default_dedup")]
    pub dedup: bool,
}

fn default_dedup() -> bool {
    true
}
