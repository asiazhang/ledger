//! OpenAPI 契约装配（`ApiDoc`）与契约自举端点（`GET /api/v1/openapi.json`）。

use axum::Json;
use axum::response::IntoResponse;
use utoipa::OpenApi;

use crate::accounts::{Account, AccountBalance, AccountInput, AccountType, AccountUpdateInput};
use crate::categories::{Category, CategoryInput};
use crate::currencies::Currency;
use crate::investment::{Instrument, InstrumentListResult, InstrumentType};
use crate::merchants::Merchant;
use crate::models::{
    CreateTransactionResult, Transaction, TransactionBatchInput, TransactionInput,
    TransactionListResult, UpdateTransactionInput,
};
use crate::transaction::amount::TransactionKind;

use super::error::ErrorResponse;
use super::handlers;
use super::handlers::funds::FundLookup;
use super::handlers::instruments::InstrumentCreateInput;

/// Ledger 记账 API 的 OpenAPI 契约文档。
///
/// 供 AI 编程助手读取完整端点契约（账户/分类幂等创建、批量交易去重、币种清单、导入知识）。
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ledger 记账 API",
        description = "Ledger 记账应用本地 API（基础地址 http://127.0.0.1:9527/api/v1），供 AI 编程助手读写记账数据\
                      （账户/分类/商户/交易/标的），主要场景为数据迁移，亦可直接录入。\
                      金额均以整数分（`_cents` 后缀）为单位；例外：buy/sell 的 `price_cents`（成交单价）\
                      与标的 `price_cents`（现价）以万分之一元为单位（元 × 10000，如 1.2345 元 → 12345），\
                      见 ADR-0038 价格刻度。\
                      场外基金（fund 类型标的）申赎以确认单为权威：请求体带整分金额（`amount_cents`）\
                      与确认份额（`quantity`），不带 `price_cents`——服务端按金额 ∓ 手续费 ÷ 份额反算净值。\
                      账户/分类按自然键幂等创建；交易可携带商户名字符串（后端精确匹配复用或即建）；\
                      批量交易默认去重，命中返回 `duplicate: true`。\
                      buy/sell 需携带标的 id：可先用标的搜索端点把流水中的标的描述解析为 id；\
                      场外基金例外——先按 6 位代码查询（`GET /api/v1/funds/{code}`）确认识别，\
                      再以真实代码创建标的（见导入知识「基金申赎」节）。",
        version = "0.1.0"
    ),
    paths(
        handlers::accounts::list_accounts_handler,
        handlers::accounts::create_account_handler,
        handlers::accounts::update_account_handler,
        handlers::accounts::delete_account_handler,
        handlers::accounts::list_account_balances_handler,
        handlers::categories::list_categories_handler,
        handlers::categories::create_category_handler,
        handlers::categories::delete_category_handler,
        handlers::currencies::list_currencies_handler,
        handlers::instruments::search_instruments_handler,
        handlers::instruments::create_instrument_handler,
        handlers::funds::lookup_fund_handler,
        handlers::merchants::list_merchants_handler,
        handlers::transactions::list_transactions_handler,
        handlers::transactions::batch_create_transactions_handler,
        handlers::transactions::update_transaction_handler,
        handlers::transactions::delete_transaction_handler,
        handlers::import::import_knowledge_handler
    ),
    components(schemas(
        Account,
        AccountBalance,
        AccountInput,
        AccountType,
        AccountUpdateInput,
        Category,
        CategoryInput,
        Currency,
        FundLookup,
        Instrument,
        InstrumentCreateInput,
        InstrumentListResult,
        InstrumentType,
        Merchant,
        Transaction,
        TransactionInput,
        UpdateTransactionInput,
        TransactionBatchInput,
        TransactionListResult,
        CreateTransactionResult,
        TransactionKind,
        ErrorResponse
    ))
)]
pub struct ApiDoc;

/// 生成式返回 OpenAPI 文档（机器可读契约，供 AI 查询端点结构）。
pub async fn openapi_json_handler() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}
