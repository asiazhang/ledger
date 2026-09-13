//! 交易写入入参模型（共享语义区，issue #423）：创建/修改/批量请求体与结果。
//!
//! 职责：请求体类型与 `UpdateTransactionInput → TransactionInput` 归一。不变量：幂等键
//! 不可编辑（修改路径置 None）；investment 字段契约见字段注释。ADR 指针：ADR-0113
//! 决策 8。陷阱：`merchant_name` 与 `merchant_id` 互斥，解析归写入协议。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::amount::TransactionKind;

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TransactionInput {
    /// 交易类型枚举（serde 小写字符串反序列化）。非法 kind 在反序列化阶段报 400
    /// （请求体格式错误，见 batch 端点描述）；合法值 wire 与裸 String 一致。
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: String,
    pub to_account_id: Option<String>,
    /// 可选出资账户（issue #935 / ADR-0096）：仅 buy/sell 可携带，准入收口
    /// `funding::validate_funding_account`；缺省即「结算账户 = 投资账户」。
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    /// 商户名字符串（AI 导入契约，issue #194 / ADR-0028）：提交体不带 `merchant_id` 而
    /// 带商户名时，后端写入路径精确匹配在用商户名——命中复用、未命中即建，归一化责任
    /// 收口在后端，AI 不负责商户去重。与 `merchant_id` 互斥（同时提供属请求错误）。
    pub merchant_name: Option<String>,
    /// 可选保单引用（issue #361 / ADR-0051 决策 3）：仅 expense/income 可携带，
    /// 其余 kind 行为层拒绝；引用不存在的保单返回 400（中文错误，可读回自纠）。
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    /// 标的 id（仅 buy/sell/convert/split/dividend 需提供）：先用标的搜索端点
    /// （`GET /api/v1/instruments`）把源数据中的标的描述解析为 id，未命中再按创建端点
    /// 幂等新建；引用不存在的标的返回 400（中文错误，可读回自纠）。dividend 不要求
    /// 当前有持仓，到账账户为任意在用账户（ADR-0109）。
    pub instrument_id: Option<String>,
    /// 成交数量（份，可含小数）：仅 buy/sell/convert/split 需提供。buy/sell 为成交份额、
    /// convert 为转出份额，均必须 > 0；split 为同一账户内单标的的带符号份额增量 Δ
    /// （`+` 折算/结转/送股；`−` 缩股且 |Δ| 小于当前持仓），必须 ≠ 0。
    /// split 无现金腿：`amount_cents` 恒填 0、`price_cents` 不提供、`fee_cents` 只能为 0，
    /// 不携带 `to_instrument_id` / `to_account_id` / `funding_account_id` / 商户 / 分类 / 保单，
    /// 落账前后全部账户余额不变，仅持仓份额与市值随 Δ 变化（ADR-0106 决策 1/7）。
    /// dividend 不提供数量（无份额变动）。
    pub quantity: Option<f64>,
    /// 成交单价（万分之一元，元 × 10000；价格刻度见 ADR-0038，金额列仍为整数分）：
    /// 非基金标的必填且必须 > 0；场外基金（issue #302 / ADR-0038 金额权威）不提供，
    /// 由后端按（行金额 ∓ 手续费）÷ 数量反算到万分之一元；convert 不提供（由转出金额 ÷ 转出份额反算）。
    /// dividend 不提供（无成交单价）。
    pub price_cents: Option<i64>,
    /// 手续费（整数分，可省，默认 0）：sell 不得超过卖出收入（数量 × 单价，非基金）；
    /// 服务端行金额：非基金按 buy = 数量 × 单价 + 费用、sell = 数量 × 单价 − 费用重算；
    /// 场外基金以 `amount_cents`（确认单整分金额）为权威，行金额原样采用；convert 如实记录、不进支出报表。
    /// dividend 不接受手续费（只能省或填 0）。
    pub fee_cents: Option<i64>,
    /// 转入标的 id（仅 convert）：与 `instrument_id`（转出标的）必须不同。
    pub to_instrument_id: Option<String>,
    /// 转入份额（仅 convert，必须 > 0）：转入批次建仓数量。
    pub to_quantity: Option<f64>,
    /// 转出金额（仅 convert，整数分，必须 > 0）：确认单转出端金额（列表展示口径）。
    pub out_amount_cents: Option<i64>,
    /// 转入金额（仅 convert，整数分，必须 > 0）：确认单转入端金额。
    pub in_amount_cents: Option<i64>,
    /// 客户端提供的、内容无关的导入幂等键（指向"该交易来自源文件哪一行"）。
    /// 带键时批量导入以其为准去重（同键跳过、内容无关）；无键时回退内容哈希兜底。
    pub idempotency_key: Option<String>,
}

/// 交易修改请求体（`PUT /api/v1/transactions/{id}`）。
///
/// 与 `TransactionInput` 的唯一差异是不含 `idempotency_key`：幂等键不可编辑，只在导入时落定，
/// 编辑不改变导入身份（修改后重跑同批导入仍按同键去重、不产生重复）。
/// buy/sell 仍需 `instrument_id`/`quantity`/`price_cents`/`fee_cents`；convert 的就地修改
/// （全字段替换）同样需 `instrument_id`/`quantity`/`to_instrument_id`/`to_quantity`/
/// `out_amount_cents`/`in_amount_cents`/`fee_cents`（kind 变更仍被拒，见 `behavior::update`）。
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateTransactionInput {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: String,
    pub to_account_id: Option<String>,
    /// 可选出资账户（与 `TransactionInput.funding_account_id` 同一契约）。
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    /// 商户名字符串（与 `TransactionInput.merchant_name` 同一契约）：修改路径同样
    /// 由后端精确匹配复用或即建；解析出的 id 与该行当前商户相同即视为保持历史引用。
    pub merchant_name: Option<String>,
    /// 可选保单引用（与 `TransactionInput.policy_id` 同一契约）：修改路径提交值与
    /// 该行当前保单相同视为保持历史引用（已软删保单的历史交易仍可改其他字段）。
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    /// 标的 id（仅 buy/sell/convert/split/dividend 需提供）：与 `TransactionInput` 同一契约；
    /// 引用不存在的标的返回 400（中文错误，可读回自纠）。
    pub instrument_id: Option<String>,
    /// 成交数量（份，可含小数）：与 `TransactionInput.quantity` 同一契约。
    pub quantity: Option<f64>,
    /// 成交单价（万分之一元）：与 `TransactionInput.price_cents` 同一契约，必须 > 0。
    pub price_cents: Option<i64>,
    /// 手续费（整数分，可省，默认 0）：与 `TransactionInput.fee_cents` 同一契约。
    pub fee_cents: Option<i64>,
    /// 转入标的 id（仅 convert）：不得与转出标的相同。
    pub to_instrument_id: Option<String>,
    /// 转入份额。
    pub to_quantity: Option<f64>,
    /// 转出金额（分）。
    pub out_amount_cents: Option<i64>,
    /// 转入金额（分）。
    pub in_amount_cents: Option<i64>,
}

impl From<UpdateTransactionInput> for TransactionInput {
    fn from(u: UpdateTransactionInput) -> Self {
        // 幂等键不作为可编辑字段：修改路径忽略请求中的该字段（保留既有行的幂等键），
        // 此处统一置 None 表达"不写入新幂等键"。转换两腿字段随就地修改契约携带
        // （全字段替换），kind 变更由行为层拒绝。
        TransactionInput {
            kind: u.kind,
            amount_cents: u.amount_cents,
            currency_code: u.currency_code,
            account_id: u.account_id,
            to_account_id: u.to_account_id,
            funding_account_id: u.funding_account_id,
            category_id: u.category_id,
            merchant_id: u.merchant_id,
            merchant_name: u.merchant_name,
            policy_id: u.policy_id,
            refund_of_transaction_id: u.refund_of_transaction_id,
            note: u.note,
            date: u.date,
            instrument_id: u.instrument_id,
            quantity: u.quantity,
            price_cents: u.price_cents,
            fee_cents: u.fee_cents,
            to_instrument_id: u.to_instrument_id,
            to_quantity: u.to_quantity,
            out_amount_cents: u.out_amount_cents,
            in_amount_cents: u.in_amount_cents,
            idempotency_key: None,
        }
    }
}

/// 按 kind 校验并归一化后的一笔交易行字段（供创建与修改共用）。
///
/// 创建路径据此 INSERT、修改路径据此 UPDATE —— 校验与字段解析只做一次。
/// buy/sell 的持仓/卖出关联等副作用由调用方在落库时按其身份（新增或替换）另行执行。
/// serde（issue #855）：作为交易同步命令（`TransactionCommand`）的行载荷随 op
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
