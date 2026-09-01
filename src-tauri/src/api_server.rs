use std::sync::{Arc, Mutex};

use crate::commands::investment::{
    FundCreateOutcome, create_fund_degraded, is_six_digit_code, persist_fund_detail,
    validate_fund_code,
};
use crate::commands::sync::persist::price_value_to_cents;
use crate::error::{AppError, ErrClass};
use crate::events::SignalEmitter;
use crate::models::{
    Account, AccountBalance, AccountInput, AccountType, AccountUpdateInput, Category,
    CategoryInput, CreateTransactionResult, Currency, FundDetail, Instrument, InstrumentInput,
    InstrumentListFilter, InstrumentListResult, InstrumentType, Merchant, Transaction,
    TransactionBatchInput, TransactionInput, TransactionListFilter, TransactionListResult,
    UpdateTransactionInput,
};
use crate::signals::{WriteEvidence, WriteOp, emit_for};
use crate::transaction::amount::TransactionKind;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use rusqlite::Connection;
use serde::Deserialize;
use tauri::AppHandle;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa::ToSchema;

/// 东财基金详情获取函数接缝（issue #304 / ADR-0039）：`基金代码 → Result<FundDetail>`，
/// 查无此码以 `AppError::Invalid`（中文错误）上抛——与 IPC/BDD 的
/// `add_fund_by_code_with` 注入接缝同一形状约定。生产路径为东财 FundSearchAPI
/// （`fetch_fund_detail_production`）；HTTP 集成测试以注入桩离线驱动
/// （`setup_app_with_fund_fetch`），全部基金端点集成测试不触真实网络。
pub type FundDetailFetcher = Arc<dyn Fn(&str) -> Result<FundDetail, AppError> + Send + Sync>;

/// 失效信号发射槽（壳层 handler 的提取形状，ADR-0054 #367 修订）：写事务提交
/// 成功后经信号映射单点发射失效信号的机制槽位，收口于发射器接缝
/// `events::SignalEmitter`（spec #366 固化）。`None` = 集成测试跳过发射分支；
/// 生产注入 `AppHandle`（主线程非阻塞投递实现）经未尺寸化强转装入。
pub type EmitterSlot = Option<Arc<dyn SignalEmitter>>;

/// HTTP 服务器状态：数据库连接 + 失效信号发射槽 + 可选东财基金详情接缝。
///
/// `emitter`（发射槽，ADR-0044 / ADR-0054）：`Some` 时写事务提交成功后经信号
/// 映射单点发射失效信号。生产路径由 `start_http_server` 注入
/// `Some(Arc::new(app))`——同一 `AppHandle` 发射器实现，行为与泛化前零变化；
/// 集成测试（`tests/api_server/`）不经真实 Tauri 运行时构建路由，传 `None`
/// 跳过发射分支，或注入受控发射器观察「写请求返回后信号最终到达」的
/// 外部行为（spec #367，`signal_delivery.rs`）。
///
/// `fund_fetch` 为东财基金详情获取接缝：`None` = 生产路径（真实东财，
/// `spawn_blocking` 连接锁外往返）；集成测试注入桩离线驱动（issue #304）。
#[derive(Clone)]
pub struct ApiState {
    pub conn: Arc<Mutex<Connection>>,
    pub emitter: EmitterSlot,
    pub fund_fetch: Option<FundDetailFetcher>,
}

impl FromRef<ApiState> for Arc<Mutex<Connection>> {
    fn from_ref(state: &ApiState) -> Self {
        state.conn.clone()
    }
}

impl FromRef<ApiState> for EmitterSlot {
    fn from_ref(state: &ApiState) -> Self {
        state.emitter.clone()
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Invalid(_) => StatusCode::BAD_REQUEST,
            AppError::Parse(_) => StatusCode::BAD_REQUEST,
            AppError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // 码化错误按归类取状态（ADR-0050）：Invalid→400、NotFound→404
            AppError::Coded { class, .. } => match class {
                ErrClass::Invalid => StatusCode::BAD_REQUEST,
                ErrClass::NotFound => StatusCode::NOT_FOUND,
            },
        };
        (status, Json(self)).into_response()
    }
}

/// 统一错误响应格式：`{ "kind": "<ErrorKind>", "message": "<中文描述>" }`；
/// 码化错误额外携带稳定 `code` 与可选 `params`（issue #342 二期 / ADR-0050，只增不改）。
#[derive(ToSchema)]
#[allow(dead_code)]
struct ErrorResponse {
    /// 错误类型枚举：`Db` / `NotFound` / `Invalid` / `Parse` / `Io`
    kind: String,
    /// 中文错误描述
    message: String,
    /// 稳定错误码（可选，仅码化错误与系统类错误携带），领域语言命名如 `transfer.to-account-required`
    code: Option<String>,
    /// 插值参数（可选，按消息中动态值出现顺序）
    params: Option<Vec<String>>,
}

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
        list_accounts_handler,
        create_account_handler,
        update_account_handler,
        delete_account_handler,
        list_account_balances_handler,
        list_categories_handler,
        create_category_handler,
        delete_category_handler,
        list_currencies_handler,
        search_instruments_handler,
        create_instrument_handler,
        lookup_fund_handler,
        list_merchants_handler,
        list_transactions_handler,
        batch_create_transactions_handler,
        update_transaction_handler,
        delete_transaction_handler,
        import_knowledge_handler
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
pub(crate) struct ApiDoc;

/// 生成式返回 OpenAPI 文档（机器可读契约，供 AI 查询端点结构）。
async fn openapi_json_handler() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

pub fn build_router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/openapi.json", get(openapi_json_handler))
        .route(
            "/api/v1/accounts",
            get(list_accounts_handler).post(create_account_handler),
        )
        .route(
            "/api/v1/accounts/{id}",
            put(update_account_handler).delete(delete_account_handler),
        )
        .route(
            "/api/v1/accounts/balances",
            get(list_account_balances_handler),
        )
        .route(
            "/api/v1/categories",
            get(list_categories_handler).post(create_category_handler),
        )
        .route("/api/v1/categories/{id}", delete(delete_category_handler))
        .route("/api/v1/transactions", get(list_transactions_handler))
        .route(
            "/api/v1/transactions/batch",
            post(batch_create_transactions_handler),
        )
        .route(
            "/api/v1/transactions/{id}",
            put(update_transaction_handler).delete(delete_transaction_handler),
        )
        .route("/api/v1/currencies", get(list_currencies_handler))
        .route(
            "/api/v1/instruments",
            get(search_instruments_handler).post(create_instrument_handler),
        )
        .route("/api/v1/funds/{code}", get(lookup_fund_handler))
        .route("/api/v1/merchants", get(list_merchants_handler))
        .route("/api/v1/import/knowledge", get(import_knowledge_handler))
        // 注意：axum 的 `Router::layer` 只包裹“当前已有的” route——若在声明任何 route
        // 之前调用，会对空路由集合空操作（`route_layer` 则会在无 route 时 panic，强制先
        // 声明 route 再加层）。所以 `TraceLayer` 必须放在所有 route 声明之后，否则其请求
        // span 空转，导入路径 SQL 的归因不生效（issue #44）。
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub fn start_http_server(app: AppHandle, state: Arc<Mutex<Connection>>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("创建 Tokio 运行时失败");
        rt.block_on(async move {
            let router = build_router(ApiState {
                conn: state,
                emitter: Some(Arc::new(app)),
                fund_fetch: None,
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:9527")
                .await
                .expect("绑定 9527 端口失败");
            println!("Ledger API 服务已启动: http://127.0.0.1:9527");
            axum::serve(listener, router)
                .await
                .expect("HTTP 服务器异常退出");
        });
    });
}

/// HTTP 壳「端点 → 写操作身份」声明表（ADR-0044 决策 3 / #335）：本壳数据面
/// （OpenAPI 契约自描述枚举的 method + path 端点集，与 `#[utoipa::path]` 注解同源）
/// 的逐一身份声明——写端点声明其 [`WriteOp`]（与 handler 内 `emit_after_write` 的
/// 判定键同源），读端点显式 [`None`]（刻意无写身份，是决策而非遗漏）。
///
/// 交叉核对测试（`signals_cross_check`）以 [`ApiDoc::openapi`] 枚举的端点集与本表
/// 双向比对：「新写端点忘了声明身份」「表键漂移」「契约漏记端点」测试期即红；
/// `Some` 身份再经 `signals_for` 的编译期穷尽 match 保证映射不缺行。
/// `GET /api/v1/openapi.json` 是契约自举端点（文档自身不入契约），不持数据身份、不入表。
///
/// **新增 / 删除 / 改动端点必须同步本表**——漏声明不是「不发信号」的合法形态。
pub const HTTP_ENDPOINT_WRITE_OPS: &[(&str, Option<WriteOp>)] = &[
    ("POST /api/v1/accounts", Some(WriteOp::CreateAccount)),
    ("PUT /api/v1/accounts/{id}", Some(WriteOp::UpdateAccount)),
    ("DELETE /api/v1/accounts/{id}", Some(WriteOp::DeleteAccount)),
    ("GET /api/v1/accounts", None),
    ("GET /api/v1/accounts/balances", None),
    ("GET /api/v1/categories", None),
    ("POST /api/v1/categories", Some(WriteOp::CreateCategory)),
    (
        "DELETE /api/v1/categories/{id}",
        Some(WriteOp::DeleteCategory),
    ),
    ("GET /api/v1/currencies", None),
    ("GET /api/v1/transactions", None),
    (
        "POST /api/v1/transactions/batch",
        Some(WriteOp::BatchCreateTransactions),
    ),
    (
        "PUT /api/v1/transactions/{id}",
        Some(WriteOp::UpdateTransaction),
    ),
    (
        "DELETE /api/v1/transactions/{id}",
        Some(WriteOp::DeleteTransaction),
    ),
    ("GET /api/v1/instruments", None),
    ("POST /api/v1/instruments", Some(WriteOp::CreateInstrument)),
    ("GET /api/v1/funds/{code}", None),
    ("GET /api/v1/merchants", None),
    ("GET /api/v1/import/knowledge", None),
];

/// HTTP 壳的单点映射发射（ADR-0044）：写事务**提交成功后**按写操作身份 + 结果证据经
/// `signals` 映射单点判定「发不发、发哪个」——壳层只转发，不持有判定知识，也不再出现
/// 按端点手写的 emit 样板。发射槽为 `None`（集成测试不经真实 Tauri 运行时，见
/// [`ApiState`]）跳过发射分支，语义不变；发射失败静默忽略，不影响写结果。
fn emit_after_write(emitter: &EmitterSlot, op: WriteOp, evidence: WriteEvidence) {
    if let Some(emitter) = emitter {
        emit_for(emitter.as_ref(), op, evidence);
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/accounts",
    tag = "accounts",
    summary = "列出所有账户",
    description = "返回账户完整列表，**包含预置黑洞账户**（`is_hidden=true`）。\
                  AI 可把 `资金账户=无` 的交易映射到对应币种的黑洞账户。",
    responses(
        (status = 200, description = "账户列表（含黑洞账户）", body = [Account]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn list_accounts_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Account>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let accounts = crate::commands::list_accounts_for_api_internal(&conn)?;
    Ok(Json(accounts))
}

#[utoipa::path(
    post,
    path = "/api/v1/accounts",
    tag = "accounts",
    summary = "创建账户（按自然键幂等）",
    description = "按 `name` + `type` + `currency_code` 幂等创建账户：已存在同名同类型同币种且未删除的账户时，\
                  直接返回已有账户的 `id`，不重复插入、不报错。",
    request_body = AccountInput,
    responses(
        (status = 201, description = "创建成功，返回账户 ID", body = String),
        (status = 400, description = "参数错误", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn create_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Json(input): Json<AccountInput>,
) -> Result<(StatusCode, Json<String>), AppError> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = crate::db::write(&conn, |conn| {
        crate::commands::create_account_idempotent_internal(conn, input)
    })?;
    emit_after_write(&emitter, WriteOp::CreateAccount, WriteEvidence::None);
    Ok((StatusCode::CREATED, Json(id)))
}

#[utoipa::path(
    put,
    path = "/api/v1/accounts/{id}",
    tag = "accounts",
    summary = "编辑账户（改名/改币种）",
    description = "按 `id` 编辑账户，可选字段未传保持原值。`type` 不可改（参与余额符号归属）；\
                  `currency_code` 仅无交易账户可改（有交易时后端拒绝，避免历史折算口径错乱）。\
                  余额校准不走本端点：创建一笔与黑洞账户的转账即可（余额 = 期初 + Σ 流水）。\
                  不存在的 id 返回 404。成功返回 200 与更新后的完整账户。",
    request_body = AccountUpdateInput,
    params(
        ("id" = String, Path, description = "账户 ID")
    ),
    responses(
        (status = 200, description = "更新后的完整账户", body = Account),
        (status = 400, description = "参数错误（如名称为空、有交易改币种、未知币种）", body = ErrorResponse),
        (status = 404, description = "账户不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn update_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
    Json(input): Json<AccountUpdateInput>,
) -> Result<Json<Account>, AppError> {
    // 连接层统一写入口（ADR-0032）：修改与读回同一写闭包，提交点置脏/检查单点。
    let updated = crate::db::write(&conn, |conn| {
        crate::commands::update_account_internal(conn, &id, input)?;
        crate::commands::get_account_internal(conn, &id)
    })?;
    emit_after_write(&emitter, WriteOp::UpdateAccount, WriteEvidence::None);
    Ok(Json(updated))
}

#[utoipa::path(
    delete,
    path = "/api/v1/accounts/{id}",
    tag = "accounts",
    summary = "删除账户（软删除）",
    description = "按 `id` 软删除账户（`is_deleted=1`）。**不校验引用**（与 UI 行为一致：删除有交易的账户后历史交易仍保留）。\
                  不存在的 id 返回 404。成功返回 204 No Content。",
    params(
        ("id" = String, Path, description = "账户 ID")
    ),
    responses(
        (status = 204, description = "删除成功（无响应体）"),
        (status = 404, description = "账户不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn delete_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    crate::db::write(&conn, |conn| {
        crate::commands::delete_account_internal(conn, &id)
    })?;
    emit_after_write(&emitter, WriteOp::DeleteAccount, WriteEvidence::None);
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get,
    path = "/api/v1/accounts/balances",
    tag = "accounts",
    summary = "列出全部未删除账户的实时余额（含黑洞账户）",
    description = "返回 `{account, balance_cents}[]`，与 AI 侧账户列表一致**包含 `is_hidden` 黑洞账户**。\
                  余额口径 = 初始余额 + 收入 − 支出 + 转入 − 转出 + 退款，实时计算不持久化。\
                  软删除账户不在列表中。转账分别计入转出与转入账户。",
    responses(
        (status = 200, description = "账户余额列表（含黑洞账户）", body = [AccountBalance]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn list_account_balances_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<AccountBalance>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let balances = crate::commands::list_account_balances_for_api_internal(&conn)?;
    Ok(Json(balances))
}

#[utoipa::path(
    get,
    path = "/api/v1/categories",
    tag = "categories",
    summary = "列出所有分类",
    description = "返回全部分类（含种子数据），`kind` 为 `income` / `expense`，支持两级分类体系（`parent_id`）。",
    responses(
        (status = 200, description = "分类列表", body = [Category]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn list_categories_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<crate::models::Category>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::commands::list_categories_internal(&conn)?))
}

#[utoipa::path(
    post,
    path = "/api/v1/categories",
    tag = "categories",
    summary = "创建分类（按自然键幂等）",
    description = "按 `name` + `kind` + `parent_id` 幂等创建分类：已存在同名同类型同父分类且未删除的分类时，\
                  直接返回已有分类的 `id`，不重复插入、不报错。`parent_id` 为 `null` 表示顶级分类。",
    request_body = CategoryInput,
    responses(
        (status = 201, description = "创建成功，返回分类 ID", body = String),
        (status = 400, description = "参数错误", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn create_category_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    body: String,
) -> Result<(StatusCode, Json<String>), AppError> {
    let input: CategoryInput =
        serde_json::from_str(&body).map_err(|e| AppError::Invalid(e.to_string()))?;
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = crate::db::write(&conn, |conn| {
        crate::commands::create_category_idempotent_internal(conn, input)
    })?;
    emit_after_write(&emitter, WriteOp::CreateCategory, WriteEvidence::None);
    Ok((StatusCode::CREATED, Json(id)))
}

#[utoipa::path(
    delete,
    path = "/api/v1/categories/{id}",
    tag = "categories",
    summary = "删除分类（软删除）",
    description = "按 `id` 软删除分类（`is_deleted=1`）。**不校验引用**（与 UI 行为一致）。\
                  不存在的 id 返回 404。成功返回 204 No Content。",
    params(
        ("id" = String, Path, description = "分类 ID")
    ),
    responses(
        (status = 204, description = "删除成功（无响应体）"),
        (status = 404, description = "分类不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn delete_category_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    crate::db::write(&conn, |conn| {
        crate::commands::delete_category_internal(conn, &id)
    })?;
    emit_after_write(&emitter, WriteOp::DeleteCategory, WriteEvidence::None);
    Ok(StatusCode::NO_CONTENT)
}

/// 商户列表（AI 导入契约，issue #194 / ADR-0028）：供 AI 在提交交易前拉取在用商户，
/// 按已有名字填 `merchant_name` 复用字典（避免同义名分裂商户字典）。仅返回在用行
/// （`is_deleted=0`，与 IPC `list_merchants` 缺省一致）；软删商户不可再被新交易选择。
#[utoipa::path(
    get,
    path = "/api/v1/merchants",
    tag = "merchants",
    summary = "列出所有在用商户",
    description = "返回商户字典的全部在用行（`is_deleted=0`），按名称排序。\
                  提交交易时可带 `merchant_name`（商户名字符串）：后端按名字精确匹配在用商户，\
                  命中复用、未命中即建，AI 无需自行去重；建议先拉取本列表、按已有名字提交，\
                  避免同义名分裂商户字典。仅 `income`/`expense` 可携带商户（refund 自动继承原支出商户）。",
    responses(
        (status = 200, description = "在用商户列表", body = [Merchant]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn list_merchants_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Merchant>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::commands::list_merchants_internal(
        &conn, false,
    )?))
}

/// 标的搜索查询参数（`GET /api/v1/instruments`，issue #294 / ADR-0037）。
#[derive(Debug, Deserialize)]
struct InstrumentSearchQuery {
    /// 搜索关键词（必填；空即 400——搜索式而非全量列表）
    query: Option<String>,
    /// 返回条数上限：缺省 20，最大 100（超出收敛为 100，小于 1 视为 1）
    limit: Option<i64>,
    /// 交易市场精确过滤（sh / sz / hk / unknown）
    market: Option<String>,
    /// 标的类型过滤（stock/fund/bond/etf/other）：同码异类型消歧用
    #[serde(rename = "type")]
    kind: Option<InstrumentType>,
}

/// 标的搜索（AI 导入契约，issue #294 / ADR-0037）：供 AI 把流水中的标的描述
/// （代码/名称/拼音首字母）解析为可用标的 id，不提供全量列表。语义复用
/// ADR-0027 统一模糊搜索（既有标的搜索接缝 `list_instruments_internal`），
/// 不为 AI 另造第二口径；按 symbol 排序，封顶返回 + 命中总数控制上下文预算。
#[utoipa::path(
    get,
    path = "/api/v1/instruments",
    tag = "instruments",
    summary = "按关键词搜索标的（统一模糊搜索、封顶返回）",
    description = "返回 `{items, total}`：`items` 为按 symbol 排序的前 `limit` 条命中标的，\
                  `total` 恒为命中总数。`query` 必填（缺失或纯空白返回 400——本端点是搜索式而非全量列表）；\
                  `limit` 缺省 20、上限 100（超出收敛为 100）。\
                  命中语义为统一模糊搜索：`query` 按空白切词、词条之间 AND；每个词条对「代码 · 名称」label 判定——\
                  原文连续子串 ∨ 拼音首字母串子序列（均大小写不敏感；无名称标的退化为裸代码），\
                  如 `gzmt` 命中「600519 贵州茅台」。\
                  `market` / `type` 可选精确过滤；同码异类型（如基金 000001 vs 股票 000001）\
                  靠 `type` 消歧。返回完整 Instrument 形状（含 `price_cents` 最新行情与 `invested` 是否持仓）。",
    params(
        ("query" = String, Query, description = "搜索关键词（必填，空即 400）"),
        ("limit" = Option<i64>, Query, description = "返回条数上限，缺省 20，最大 100（小于 1 视为 1）"),
        ("market" = Option<String>, Query, description = "交易市场精确过滤（sh / sz / hk / unknown）"),
        ("type" = InstrumentType, Query, description = "标的类型过滤（stock/fund/bond/etf/other），同码异类型消歧用")
    ),
    responses(
        (status = 200, description = "命中标的 {items, total}", body = InstrumentListResult),
        (status = 400, description = "缺 query 或 query 为纯空白；或参数非法", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn search_instruments_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    Query(params): Query<InstrumentSearchQuery>,
) -> Result<Json<InstrumentListResult>, AppError> {
    // query 必填（trim 后为空视同缺失）：显式校验以返回统一 `{kind, message}` 中文错误。
    // 参数格式错误（如 type 非法枚举值）由 axum extractor 拒绝、同样返回 400，
    // 但响应体为其默认格式（与既有 list_transactions 先例一致）。
    let query = params
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or_else(|| {
            AppError::Invalid(
                "query 不能为空：标的搜索为搜索式端点，请携带关键词（不做全量列表）".into(),
            )
        })?;
    // 封顶返回：缺省 20、上限收敛 100，AI 上下文预算可控。
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let filter = InstrumentListFilter {
        search: Some(query.to_string()),
        market: params.market.filter(|m| !m.is_empty()),
        kind: params.kind,
        only_invested: None,
        page: Some(1),
        page_size: Some(limit as usize),
    };
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::commands::list_instruments_internal(
        &conn, &filter,
    )?))
}

/// 标的创建请求体（`POST /api/v1/instruments`，issue #296 / ADR-0037）。
///
/// 与 IPC 侧 `InstrumentInput` 的差异仅在报价币种可省：缺省按市场推导
/// （沪深→CNY、港→HKD、未知→CNY），显式传参可覆盖。
#[derive(Debug, Deserialize, ToSchema)]
struct InstrumentCreateInput {
    /// 标的代码（必填；源数据只有名称时以名称充当代码，ADR-0037 决策 3）
    symbol: String,
    /// 标的类型（闭集：stock/fund/bond/etf/other；五类全开，不经自建标的的 UI 白名单）
    #[serde(rename = "type")]
    kind: InstrumentType,
    /// 标的名称（可选）
    name: Option<String>,
    /// 交易市场（可选，缺省 unknown；sh / sz / hk）
    market: Option<String>,
    /// 报价币种（可选；缺省按市场推导：沪深→CNY、港→HKD、未知→CNY）
    currency_code: Option<String>,
}

/// 报价币种缺省推导（ADR-0037 决策 2）：沪深→人民币、港→港币、其余（含 unknown）→人民币。
///
/// 依据：标的币种不参与买卖账务（持仓批次成本币种 = 账户币种），仅影响行情/市值折算展示。
/// 与同步侧 `commands::sync::http::MARKETS` 的 market→currency 对应（该表为同步
/// 市场闭集、模块私有，本端点按 ADR 独立定义并多担 unknown 缺省）；新增市场时两处同改。
fn derive_quote_currency(market: &str) -> &'static str {
    match market {
        "hk" => "HKD",
        // 沪深与未知市场均落人民币
        _ => "CNY",
    }
}

/// 标的幂等创建（AI 导入契约，issue #296 / ADR-0037）：find-or-create 自然键
/// （symbol, 类型），命中静默复用并按需更新名称/市场、返回既有 id，未命中创建；
/// 重复创建同一标的返回同一 id，不产生字典碎片。核心语义复用 IPC 共用创建函数
/// `create_instrument_internal`（不经自建标的的 UI 类型白名单，ADR-0037 决策 4），
/// 新建行来源标记 = `'manual'`（非同步即手动）。
#[utoipa::path(
    post,
    path = "/api/v1/instruments",
    tag = "instruments",
    summary = "创建标的（按（代码，类型）幂等 find-or-create，fund 类型经东财增强）",
    description = "按自然键（`symbol` + `type`）幂等创建标的：已存在同码同类型行时**静默复用**\
                  并按需更新名称/市场、返回既有 id，未命中创建新行（来源标记 = `manual`）——\
                  重复创建同一标的返回同一 id，不产生字典碎片。响应照账户/分类创建先例：\
                  201 + 裸 id 字符串，无 created 标记。\
                  入参：`symbol` 必填（源数据只有名称时以名称充当代码）；`type` 为闭集五类\
                  （stock/fund/bond/etf/other，五类全开）；`name` 可选；`market` 可选（缺省 `unknown`）；\
                  `currency_code` 可选（缺省按市场推导：沪深→CNY、港→HKD、未知→CNY，显式传参可覆盖）。\
                  **fund 类型增强**：`symbol` 为真实 6 位代码时后端经东方财富校验并回填权威名称、\
                  落最新净值现价；查无此码返回 400 拒绝创建；东财网络不可达时降级为提交名称 + 真实代码建行\
                  （不阻塞导入）；非 6 位 symbol（名称充代码，仅限源数据无代码）不触发校验、不进净值通道。\
                  fund + 6 位代码分支的字典形态收口：显式 `market` / `currency_code` 不生效（恒 unknown / 人民币）。\
                  建议先搜索（GET /api/v1/instruments）无命中再创建，防同义标的碎片；\
                  基金先按代码查询（GET /api/v1/funds/{code}）确认识别，必带真实 6 位代码。",
    request_body = InstrumentCreateInput,
    responses(
        (status = 201, description = "创建或命中复用，返回标的 ID", body = String),
        (status = 400, description = "参数错误（如标的代码为空）", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn create_instrument_handler(
    State(state): State<ApiState>,
    Json(input): Json<InstrumentCreateInput>,
) -> Result<(StatusCode, Json<String>), AppError> {
    // fund 增强的东财往返判定（ADR-0039 决策 3）：仅 fund + 真实 6 位代码触发；
    // 名称充代码（非 6 位，仅限源数据无代码）与其他类型不发起网络请求。
    enum Enrichment {
        Authoritative(FundDetail),
        Degrade,
    }
    let enrichment: Option<Enrichment> =
        if input.kind == InstrumentType::Fund && is_six_digit_code(&input.symbol) {
            Some(
                match fetch_fund_detail_for_api(&state, &input.symbol).await {
                    // 东财命中：权威名称回填 + 净值落现价。
                    Ok(detail) => Enrichment::Authoritative(detail),
                    // 查无此码（接缝约定以 Invalid 上抛）：显式拒绝创建，AI 可提示用户或跳过该行。
                    Err(e @ AppError::Invalid(_)) => return Err(e),
                    // 网络不可达等临时故障：降级为 AI 提供名称 + 真实代码建行，不阻塞导入。
                    Err(_) => Enrichment::Degrade,
                },
            )
        } else {
            None
        };
    // 报价币种可省：缺省按市场推导（沪深→CNY、港→HKD、未知→CNY，ADR-0037 决策 2）；
    // market 缺省解析（None→unknown）由核心创建函数单点承担，此处仅按同口径推导币种。
    // fund 增强分支不经此推导：字典形态收口为按代码即拉同款（市场 unknown、币种人民币）。
    let currency_code = input.currency_code.unwrap_or_else(|| {
        derive_quote_currency(input.market.as_deref().unwrap_or("unknown")).to_string()
    });
    // 连接层统一写入口（ADR-0032）：find-or-create 与信息更新同一写闭包，提交点置脏单点；
    // 东财往返已在锁外完成，写闭包内零网络。泛型入参仅泛型分支消费，惰性构造。
    let outcome: FundCreateOutcome = crate::db::write(&state.conn, |conn| match &enrichment {
        Some(Enrichment::Authoritative(detail)) => {
            // 东财命中：与按代码即拉同一落库接缝（权威名称回填 + 净值落现价）。
            let r = persist_fund_detail(conn, &input.symbol, detail)?;
            Ok(FundCreateOutcome {
                instrument_id: r.instrument_id,
                price_written: r.price_written,
            })
        }
        Some(Enrichment::Degrade) => create_fund_degraded(conn, &input.symbol, input.name.clone()),
        None => {
            let generic_input = InstrumentInput {
                symbol: input.symbol.clone(),
                kind: input.kind,
                name: input.name.clone(),
                currency_code,
                market: input.market.clone(),
            };
            Ok(FundCreateOutcome {
                instrument_id: crate::commands::create_instrument_internal(conn, generic_input)?,
                price_written: false,
            })
        }
    })?;
    // 落现价即广播价格失效信号（ADR-0031，与按代码即拉 IPC 命令同一信号语义；
    // 零变化不广播）；「发不发」经映射单点判定（ADR-0044），证据 = 基金增强分支是否落现价。
    emit_after_write(
        &state.emitter,
        WriteOp::CreateInstrument,
        WriteEvidence::PriceWritten(outcome.price_written),
    );
    Ok((StatusCode::CREATED, Json(outcome.instrument_id)))
}

/// 东财基金详情获取（查询与创建两端点共用，issue #304）：测试注入桩直接同步
/// 调用（离线驱动）；生产路径经 `spawn_blocking` 在连接锁外完成阻塞网络往返
/// （单请求叠加限流冷却重试最长可达分钟级，先例：`add_fund_by_code` 命令的
/// 网络拉取在锁外完成，不阻塞其它命令）。
async fn fetch_fund_detail_for_api(state: &ApiState, code: &str) -> Result<FundDetail, AppError> {
    match &state.fund_fetch {
        Some(fetch) => fetch(code),
        None => {
            let code = code.to_string();
            tauri::async_runtime::spawn_blocking(move || {
                crate::commands::sync::fund::fetch_fund_detail_production(&code)
            })
            .await
            .map_err(|e| AppError::Io(format!("基金详情查询任务执行失败: {e}")))?
        }
    }
}

/// 基金查询响应（`GET /api/v1/funds/{code}`，issue #304 / ADR-0039 决策 2）：
/// 东财详情投影为 API 价格刻度（净值 万分之一元），AI 供校验「代码 → 名称」
/// 映射与查最新净值。
#[derive(Debug, serde::Serialize, ToSchema)]
struct FundLookup {
    /// 基金代码（6 位数字）
    code: String,
    /// 东财权威名称（如「华夏成长混合」）
    name: String,
    /// 东财基金分类（如「混合型-灵活」）
    fund_class: String,
    /// 最新单位净值（万分之一元，元 × 10000，ADR-0038 价格刻度）；未公布为 null
    nav_cents: Option<i64>,
    /// 净值日期（ISO 日期）；未公布为 null
    nav_date: Option<String>,
}

impl From<FundDetail> for FundLookup {
    fn from(d: FundDetail) -> Self {
        // 净值对（值 + 日期）在东财访问层已保证成对出现（任一缺省即 nav = None）。
        Self {
            code: d.code,
            name: d.name,
            fund_class: d.fund_class,
            nav_cents: d.nav.as_ref().map(|n| price_value_to_cents(n.nav)),
            nav_date: d.nav.map(|n| n.nav_date),
        }
    }
}

/// 按代码查询场外基金（AI 导入契约，issue #304 / ADR-0039 决策 2）：只读，
/// 实时从东方财富取名称、基金类型、最新单位净值与净值日期，供 AI 校验「代码 →
/// 名称」映射与查净值。代码格式非法即刻拒绝不发起网络；查无此码返回中文错误，
/// AI 可提示用户或跳过该行。
#[utoipa::path(
    get,
    path = "/api/v1/funds/{code}",
    tag = "funds",
    summary = "按 6 位代码查询场外基金（只读，东财实时）",
    description = "返回东财实时详情：`code` / `name`（权威名称）/ `fund_class`（东财基金分类，\
                  如「混合型-灵活」）/ `nav_cents`（最新单位净值，万分之一元，元 × 10000）/\
                  `nav_date`（净值日期，ISO 日期）；基金未公布净值时后两字段为 null。\
                  `code` 必须为 6 位数字（非 6 位返回 400，不发起网络请求）；查无此码返回 400 中文错误。\
                  本端点实时访问东方财富，网络故障返回 500。\
                  基金申赎迁移时先按本端点确认识别，再以真实 6 位代码创建标的\
                  （见 `POST /api/v1/instruments` 的 fund 增强与导入知识「基金申赎」节），\
                  不走名称充代码。",
    params(
        ("code" = String, Path, description = "基金代码（6 位数字）")
    ),
    responses(
        (status = 200, description = "基金详情（名称/分类/最新净值/净值日期）", body = FundLookup),
        (status = 400, description = "代码格式非法（非 6 位数字）或查无此码", body = ErrorResponse),
        (status = 500, description = "东财网络不可达等临时故障", body = ErrorResponse)
    )
)]
async fn lookup_fund_handler(
    State(state): State<ApiState>,
    Path(code): Path<String>,
) -> Result<Json<FundLookup>, AppError> {
    // 格式非法即刻拒绝，不发起网络请求（与按代码即拉同一校验、同一中文错误）。
    validate_fund_code(&code)?;
    let detail = fetch_fund_detail_for_api(&state, &code).await?;
    Ok(Json(FundLookup::from(detail)))
}

#[utoipa::path(
    get,
    path = "/api/v1/currencies",
    tag = "currencies",
    summary = "列出所有币种",
    description = "返回全部种子币种清单（含 `人民币→CNY`、`港币→HKD`）。\
                  导入时可用它把源数据的中文币种名映射为 `currency_code`。",
    responses(
        (status = 200, description = "币种清单", body = [Currency]),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn list_currencies_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Currency>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::commands::currencies::list_currencies_internal(
        &conn,
    )?))
}

#[utoipa::path(
    get,
    path = "/api/v1/transactions",
    tag = "transactions",
    summary = "列出交易（可按日期/账户/类型过滤 + 服务端分页）",
    description = "返回 `{items, total}`：`items` 为当前页未删除交易，`total` 恒为满足过滤条件的未删除交易总数。\
                  默认按 `date DESC, created_at DESC, id DESC` 确定性排序（同日期同时间戳时按 id 稳定，翻页无重复无遗漏）。\
                  查询参数均为可选：`from`/`to`（YYYY-MM-DD 闭区间）、`account_id`（转出账户）、\
                  `involving_account_id`（涉及账户：`account_id` 或 `to_account_id` 命中即算，含转入的转账）、\
                  `merchant_id`（按商户过滤，含软删商户的历史交易）、`kind`（income/expense/transfer/buy/sell/refund，闭集枚举，非法值返回 4xx）、`page`（从 1 起，默认 1）、\
                  `page_size`（每页条数，缺省返回全部）、`limit`（取前 N 条，与分页互斥：传 `page_size` 时分页生效）。",
    params(
        ("from" = Option<String>, Query, description = "起始日期（含），YYYY-MM-DD"),
        ("to" = Option<String>, Query, description = "结束日期（含），YYYY-MM-DD"),
        ("account_id" = Option<String>, Query, description = "按转出账户过滤"),
        ("involving_account_id" = Option<String>, Query, description = "涉及账户过滤（account_id 或 to_account_id 命中即算，含转入的转账）"),
        ("merchant_id" = Option<String>, Query, description = "按商户过滤（含软删商户的历史交易）"),
        ("kind" = Option<TransactionKind>, Query, description = "income / expense / transfer / buy / sell / refund（闭集枚举，非法值 4xx）"),
        ("limit" = Option<i64>, Query, description = "取前 N 条，缺省返回全部；传 page_size 时分页路径生效"),
        ("page" = Option<usize>, Query, description = "页码，从 1 开始，默认 1"),
        ("page_size" = Option<usize>, Query, description = "每页条数，缺省返回全部（total 恒返回）")
    ),
    responses(
        (status = 200, description = "交易分页结果 {items, total}", body = TransactionListResult),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn list_transactions_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    Query(query): Query<TransactionListFilter>,
) -> Result<Json<TransactionListResult>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let result = crate::commands::list_transactions_internal(&conn, &query)?;
    Ok(Json(result))
}

#[utoipa::path(
    post,
    path = "/api/v1/transactions/batch",
    tag = "transactions",
    summary = "批量创建交易（默认去重）",
    description = "请求体为 `{ \"transactions\": TransactionInput[], \"dedup\": bool }`，`dedup` 默认 `true`。\
                  去重以交易身份为准：若一行携带 `idempotency_key`，则按该幂等键去重（内容无关——同键重跑\
                  跳过、同键但本轮内容不同仍跳过；不同键但内容完全相同则都保留），命中已存在（`is_deleted=0`）\
                  交易返回 `{success: true, duplicate: true, id: <已有 id>}`；命中查询走部分唯一索引，非全表扫描。\
                  不带幂等键的行回退到确定性内容哈希 `sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`\
                  去重（冻结契约，命中返回 `id: null`）。行可携带 `merchant_name`（商户名字符串，与 `merchant_id` 互斥）：\
                  后端精确匹配在用商户名，命中复用、未命中即建；幂等重放不产生碎商户。单条业务校验失败（金额/转账/退款/商户/标的不存在等）返回 `success: false` 并附带 `error`，不影响其他交易；\
                  `kind` 为闭集枚举（income/expense/transfer/refund/buy/sell/dividend/split），非法 kind 属请求体格式错误，整批返回 4xx。",
    request_body = TransactionBatchInput,
    responses(
        (status = 200, description = "逐条创建结果（含 duplicate 标记）", body = [CreateTransactionResult]),
        (status = 400, description = "请求体格式错误", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn batch_create_transactions_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Json(body): Json<TransactionBatchInput>,
) -> Result<Json<Vec<CreateTransactionResult>>, AppError> {
    // 连接层统一写入口（ADR-0032，issue #245）：批次事务由 run 自持，提交点置脏/
    // 到期检查单点；整批回滚不置脏由写入口闭包失败语义保证。
    let outcome = crate::db::write(&conn, |conn| {
        crate::commands::batch::TransactionBatch::run(conn, body.transactions, body.dedup)
    })?;
    // 信号在写事务提交成功后发射（映射单点判定，ADR-0044）：批内任一行即建商户才发
    // 参考失效信号（修复 HTTP 导入即建商户后前端商户字典陈旧，issue #331）。
    emit_after_write(&emitter, WriteOp::BatchCreateTransactions, outcome.evidence);
    Ok(Json(outcome.results))
}

#[utoipa::path(
    put,
    path = "/api/v1/transactions/{id}",
    tag = "transactions",
    summary = "按 id 全字段替换交易（编辑）",
    description = "按 `id` 全字段替换一笔交易，复用与创建一致的按 kind 校验（buy/refund/transfer 的关联约束一致）。\
                  `idempotency_key` 不作为可编辑字段（不在请求体中）：编辑不重算去重身份，修改后重跑同批导入\
                  仍按同键去重、不产生重复。buy/sell 的持仓/卖出关联同步重建；已有部分卖出的买入拒绝修改。\
                  不存在的 id 返回 404。成功返回 200 与更新后的完整交易。",
    request_body = UpdateTransactionInput,
    params(
        ("id" = String, Path, description = "交易 ID")
    ),
    responses(
        (status = 200, description = "更新后的完整交易", body = Transaction),
        (status = 400, description = "参数错误（如转账缺目标账户、部分卖出的买入、买卖标的不存在）", body = ErrorResponse),
        (status = 404, description = "交易不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn update_transaction_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    State(emitter): State<EmitterSlot>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTransactionInput>,
) -> Result<Json<Transaction>, AppError> {
    // 连接层统一写入口（ADR-0032）：修改与读回同一写闭包，提交点置脏/检查单点。
    let (evidence, updated) = crate::db::write(&conn, |conn| {
        let evidence = crate::commands::update_transaction_internal(conn, &id, input.into())?;
        let updated = crate::commands::get_transaction_internal(conn, &id)?;
        Ok((evidence, updated))
    })?;
    // 仅即建商户发参考失效信号（ADR-0044，issue #331）。
    emit_after_write(&emitter, WriteOp::UpdateTransaction, evidence);
    Ok(Json(updated))
}

#[utoipa::path(
    delete,
    path = "/api/v1/transactions/{id}",
    tag = "transactions",
    summary = "删除交易（软删除）",
    description = "按 `id` 软删除交易（`is_deleted=1`）。buy 交易同步清理关联持仓\
                  （`security_lots` / `security_transactions`）；若该买入已有部分卖出则返回 400。\
                  删除后该交易不再占用去重位，重跑批量导入会重新写入（`duplicate: false`）。\
                  不存在的 id 返回 404。成功返回 204 No Content。",
    params(
        ("id" = String, Path, description = "交易 ID")
    ),
    responses(
        (status = 204, description = "删除成功（无响应体）"),
        (status = 400, description = "该买入交易已有部分卖出，无法删除", body = ErrorResponse),
        (status = 404, description = "交易不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn delete_transaction_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    crate::db::write(&conn, |conn| {
        crate::commands::delete_transaction_internal(conn, &id)
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// LLM 导入知识（纯文本），导入全流程约定的单一权威（拆行、幂等去重、对账、纠错）。
///
/// 本知识是导入约定的确定性权威来源，拆解方式必须固定，否则 `dedup_hash` 漂移。
/// AI 按入口提示词指引自行获取，约定细节不进提示词。
/// 端点契约（请求/响应结构）见 OpenAPI 文档。内容维护在
/// `src-tauri/prompts/import-knowledge.md`，编译期嵌入（`include_str!`）。
const IMPORT_KNOWLEDGE: &str = include_str!("../prompts/import-knowledge.md");

#[utoipa::path(
    get,
    path = "/api/v1/import/knowledge",
    tag = "import",
    summary = "获取 LLM 导入知识",
    description = "返回导入全流程约定文本（text/plain），单一权威：每行拆解、商户约定、幂等与去重、\
                  对账完成判定、对账纠错。AI 按入口提示词指引自行获取；文本内嵌 `/api/v1/openapi.json` 地址。",
    responses(
        (status = 200, description = "text/plain 格式的导入知识", content_type = "text/plain", body = String)
    )
)]
async fn import_knowledge_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        IMPORT_KNOWLEDGE,
    )
}
