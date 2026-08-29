//! 交易领域模型：交易实体、入参、归一化结果、批量导入、列表/搜索分页。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::db::query::FromRow;
use crate::error::{AppError, Result};
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer;

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
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

/// 交易搜索分页结果（服务端分页）。
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct TransactionSearchResult {
    /// 匹配交易（当前页）。
    pub items: Vec<Transaction>,
    /// 命中总数（供「命中 N 条」与分页）。
    pub total: i64,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TransactionInput {
    /// 交易类型枚举（serde 小写字符串反序列化）。非法 kind 在反序列化阶段报 400
    /// （请求体格式错误，见 batch 端点描述）；合法值 wire 与裸 String 一致。
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    /// 商户名字符串（AI 导入契约，issue #194 / ADR-0028）：提交体不带 `merchant_id` 而
    /// 带商户名时，后端写入路径精确匹配在用商户名——命中复用、未命中即建，归一化责任
    /// 收口在后端，AI 不负责商户去重。与 `merchant_id` 互斥（同时提供属请求错误）。
    pub merchant_name: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub instrument_id: Option<String>,
    pub quantity: Option<f64>,
    pub price_cents: Option<i64>,
    pub fee_cents: Option<i64>,
    /// 客户端提供的、内容无关的导入幂等键（指向"该交易来自源文件哪一行"）。
    /// 带键时批量导入以其为准去重（同键跳过、内容无关）；无键时回退内容哈希兜底。
    pub idempotency_key: Option<String>,
}

/// 交易修改请求体（`PUT /api/v1/transactions/{id}`）。
///
/// 与 `TransactionInput` 的唯一差异是不含 `idempotency_key`：幂等键不可编辑，只在导入时落定，
/// 编辑不改变导入身份（修改后重跑同批导入仍按同键去重、不产生重复）。
/// buy/sell 仍需 `instrument_id`/`quantity`/`price_cents`/`fee_cents`。
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct UpdateTransactionInput {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    /// 商户名字符串（与 `TransactionInput.merchant_name` 同一契约）：修改路径同样
    /// 由后端精确匹配复用或即建；解析出的 id 与该行当前商户相同即视为保持历史引用。
    pub merchant_name: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub instrument_id: Option<String>,
    pub quantity: Option<f64>,
    pub price_cents: Option<i64>,
    pub fee_cents: Option<i64>,
}

impl From<UpdateTransactionInput> for TransactionInput {
    fn from(u: UpdateTransactionInput) -> Self {
        // 幂等键不作为可编辑字段：修改路径忽略请求中的该字段（保留既有行的幂等键），
        // 此处统一置 None 表达"不写入新幂等键"。
        TransactionInput {
            kind: u.kind,
            amount_cents: u.amount_cents,
            currency_code: u.currency_code,
            account_id: u.account_id,
            to_account_id: u.to_account_id,
            category_id: u.category_id,
            merchant_id: u.merchant_id,
            merchant_name: u.merchant_name,
            refund_of_transaction_id: u.refund_of_transaction_id,
            note: u.note,
            date: u.date,
            instrument_id: u.instrument_id,
            quantity: u.quantity,
            price_cents: u.price_cents,
            fee_cents: u.fee_cents,
            idempotency_key: None,
        }
    }
}

/// 按 kind 校验并归一化后的一笔交易行字段（供创建与修改共用）。
///
/// 创建路径据此 INSERT、修改路径据此 UPDATE —— 校验与字段解析只做一次。
/// buy/sell 的持仓/卖出关联等副作用由调用方在落库时按其身份（新增或替换）另行执行。
#[derive(Debug, Clone)]
pub struct NormalizedTransaction {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
}

/// `NormalizedTransaction` → `writer::NormalizedRow`（交易行写入唯一权威，issue #70）。
///
/// 转换随模型定义，消费方（通用 kind 归一化后的行、buy/sell 投资层的归一化行）
/// 直接产出 [`writer::NormalizedRow`] 交给 [`writer::insert_row`]/[`writer::update_row`] 落库；
/// investment 不再反向 import transactions 模块的行更新函数（双向依赖斩断）。
/// kind 已为 [`TransactionKind`] 枚举直赋（issue #74），转换不可失败；保留 `Result` 签名
/// 以维持消费方 `?` 传播的既有形状（无多余分支）。
impl TryFrom<&NormalizedTransaction> for writer::NormalizedRow {
    type Error = AppError;

    fn try_from(norm: &NormalizedTransaction) -> Result<Self> {
        Ok(writer::NormalizedRow {
            kind: norm.kind,
            amount_cents: norm.amount_cents,
            currency_code: norm.currency_code.clone(),
            amount_native_cents: norm.amount_native_cents,
            account_id: norm.account_id.clone(),
            to_account_id: norm.to_account_id.clone(),
            category_id: norm.category_id.clone(),
            merchant_id: norm.merchant_id.clone(),
            refund_of_transaction_id: norm.refund_of_transaction_id.clone(),
            note: norm.note.clone(),
            date: norm.date.clone(),
        })
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

/// 交易列表查询过滤条件（服务端分页 + 过滤）。
///
/// 与 `InstrumentListFilter` 先例对齐（`page_size` 下划线命名，serde 保持原样透传）。
/// 分页语义：`page` 从 1 起、缺省 1；`page_size` 缺省时返回全部（`total` 恒返回）；
/// `limit` 为独立的"取前 N 条"参数（仪表盘"最近 N 条"场景），传 `page_size` 时分页路径生效。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionListFilter {
    /// 起始日期（含），YYYY-MM-DD。
    pub from: Option<String>,
    /// 结束日期（含），YYYY-MM-DD。
    pub to: Option<String>,
    /// 按转出账户过滤。
    pub account_id: Option<String>,
    /// 涉及账户过滤（v0.2.0 之后新增扩展字段）：`account_id = X OR to_account_id = X`，
    /// 命中普通交易与转账的转出/转入两侧（含转入的转账）。
    /// 已发布字段 `account_id`（仅转出账户）语义保持不变，遵守发布冻结约定。
    pub involving_account_id: Option<String>,
    /// 按商户过滤（issue #191）：命中 `merchant_id = X` 的全部未删除交易
    /// （交易行未删即命中，商户本身可已软删——软删商户的历史交易同样可过滤）。
    pub merchant_id: Option<String>,
    /// 交易类型过滤（income / expense / transfer / buy / sell / refund）。
    /// 枚举反序列化对未知值报参数错误（400），不再静默传字符串给 SQL。
    pub kind: Option<TransactionKind>,
    /// 取前 N 条（仪表盘"最近 N 条"场景），与分页互斥：传 `page_size` 时分页路径生效。
    /// 沿用 SQLite 原生语义：`limit=0` 返回空，负值无上限。
    pub limit: Option<i64>,
    /// 页码，从 1 开始，默认 1。
    pub page: Option<usize>,
    /// 每页条数，缺省返回全部（total 恒返回）；小于 1 按 1 处理。
    pub page_size: Option<usize>,
}

/// 交易列表分页结果。
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
            category_id: row.get(7)?,
            refund_of_transaction_id: row.get(8)?,
            note: row.get(9)?,
            date: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            version: row.get(13)?,
            device_id: row.get(14)?,
            is_deleted: row.get::<_, i64>(15)? != 0,
            merchant_id: row.get(16)?,
        })
    }
}
