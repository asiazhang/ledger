use std::sync::{Arc, Mutex};

use crate::error::AppError;
use crate::models::{
    Account, AccountBalance, AccountInput, AccountType, AccountUpdateInput, Category,
    CategoryInput, CreateTransactionResult, Currency, Merchant, Transaction, TransactionBatchInput,
    TransactionInput, TransactionListFilter, TransactionListResult, UpdateTransactionInput,
};
use crate::transaction::amount::TransactionKind;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::StatusCode;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use rusqlite::Connection;
use tauri::AppHandle;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa::ToSchema;

/// HTTP 服务器状态：数据库连接 + 可选 `AppHandle`（参考写入成功后发 `ledger:changed`）。
///
/// `app` 为 `Option`：集成测试（`tests/api_server/`）不经真实 Tauri 运行时构建路由，
/// 传 `None` 即跳过发射分支（后端 emit 视为薄胶，不造 AppHandle 测试桩）；
/// 生产路径由 `start_http_server` 注入 `Some(app)`。
#[derive(Clone)]
pub struct ApiState {
    pub conn: Arc<Mutex<Connection>>,
    pub app: Option<AppHandle>,
}

impl FromRef<ApiState> for Arc<Mutex<Connection>> {
    fn from_ref(state: &ApiState) -> Self {
        state.conn.clone()
    }
}

impl FromRef<ApiState> for Option<AppHandle> {
    fn from_ref(state: &ApiState) -> Self {
        state.app.clone()
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
        };
        (status, Json(self)).into_response()
    }
}

/// 统一错误响应格式：`{ "kind": "<ErrorKind>", "message": "<中文描述>" }`。
#[derive(ToSchema)]
#[allow(dead_code)]
struct ErrorResponse {
    /// 错误类型枚举：`Db` / `NotFound` / `Invalid` / `Parse` / `Io`
    kind: String,
    /// 中文错误描述
    message: String,
}

/// Ledger 记账 API 的 OpenAPI 契约文档。
///
/// 供 AI 编程助手读取完整端点契约（账户/分类幂等创建、批量交易去重、币种清单、导入知识）。
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ledger 记账 API",
        description = "Ledger 记账应用本地 API（基础地址 http://127.0.0.1:9527/api/v1），供 AI 编程助手读写记账数据\
                      （账户/分类/商户/交易），主要场景为数据迁移，亦可直接录入。\
                      所有金额均以整数分（`_cents` 后缀）为单位。\
                      账户/分类按自然键幂等创建；交易可携带商户名字符串（后端精确匹配复用或即建）；\
                      批量交易默认去重，命中返回 `duplicate: true`。",
        version = "0.1.0"
    ),
    paths(
        list_accounts_handler,
        create_account_handler,
        delete_account_handler,
        list_account_balances_handler,
        list_categories_handler,
        create_category_handler,
        delete_category_handler,
        list_currencies_handler,
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
struct ApiDoc;

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
                app: Some(app),
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
    State(app): State<Option<AppHandle>>,
    Json(input): Json<AccountInput>,
) -> Result<(StatusCode, Json<String>), AppError> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = crate::db::write(&conn, |conn| {
        crate::commands::create_account_idempotent_internal(conn, input)
    })?;
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_ledger_changed_if_present(&app);
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
    State(app): State<Option<AppHandle>>,
    Path(id): Path<String>,
    Json(input): Json<AccountUpdateInput>,
) -> Result<Json<Account>, AppError> {
    // 连接层统一写入口（ADR-0032）：修改与读回同一写闭包，提交点置脏/检查单点。
    let updated = crate::db::write(&conn, |conn| {
        crate::commands::update_account_internal(conn, &id, input)?;
        crate::commands::get_account_internal(conn, &id)
    })?;
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_ledger_changed_if_present(&app);
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
    State(app): State<Option<AppHandle>>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    crate::db::write(&conn, |conn| {
        crate::commands::delete_account_internal(conn, &id)
    })?;
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_ledger_changed_if_present(&app);
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
    State(app): State<Option<AppHandle>>,
    body: String,
) -> Result<(StatusCode, Json<String>), AppError> {
    let input: CategoryInput =
        serde_json::from_str(&body).map_err(|e| AppError::Invalid(e.to_string()))?;
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let id = crate::db::write(&conn, |conn| {
        crate::commands::create_category_idempotent_internal(conn, input)
    })?;
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_ledger_changed_if_present(&app);
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
    State(app): State<Option<AppHandle>>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    // 连接层统一写入口（ADR-0032）：删除成功即置脏。
    crate::db::write(&conn, |conn| {
        crate::commands::delete_category_internal(conn, &id)
    })?;
    // 参考写入成功 → 通知前端重拉参考数据（issue #79）
    crate::events::emit_ledger_changed_if_present(&app);
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
                  后端精确匹配在用商户名，命中复用、未命中即建；幂等重放不产生碎商户。单条业务校验失败（金额/转账/退款/商户等）返回 `success: false` 并附带 `error`，不影响其他交易；\
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
    Json(body): Json<TransactionBatchInput>,
) -> Result<Json<Vec<CreateTransactionResult>>, AppError> {
    // 连接层统一写入口（ADR-0032，issue #245）：批次事务由 run 自持，提交点置脏/
    // 到期检查单点；整批回滚不置脏由写入口闭包失败语义保证。
    let results = crate::db::write(&conn, |conn| {
        crate::commands::batch::TransactionBatch::run(conn, body.transactions, body.dedup)
    })?;
    Ok(Json(results))
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
        (status = 400, description = "参数错误（如转账缺目标账户、部分卖出的买入）", body = ErrorResponse),
        (status = 404, description = "交易不存在", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
async fn update_transaction_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTransactionInput>,
) -> Result<Json<Transaction>, AppError> {
    // 连接层统一写入口（ADR-0032）：修改与读回同一写闭包，提交点置脏/检查单点。
    let updated = crate::db::write(&conn, |conn| {
        crate::commands::update_transaction_internal(conn, &id, input.into())?;
        crate::commands::get_transaction_internal(conn, &id)
    })?;
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

/// LLM 导入知识（纯文本），供 AI 编程助手直接注入系统提示词。
///
/// 本知识是导入约定的确定性权威来源，拆解方式必须固定，否则 `dedup_hash` 漂移。
/// 端点契约（请求/响应结构）见 OpenAPI 文档。内容维护在
/// `src-tauri/prompts/import-knowledge.md`，编译期嵌入（`include_str!`）。
const IMPORT_KNOWLEDGE: &str = include_str!("../prompts/import-knowledge.md");

#[utoipa::path(
    get,
    path = "/api/v1/import/knowledge",
    tag = "import",
    summary = "获取 LLM 导入知识（可直接注入系统提示词）",
    description = "返回精简的导入约定文本（text/plain），供 AI 编程助手直接注入。\
                  覆盖 Pixiu 列映射与正负判定 kind、`A → B` 转账拆分、`无`/`→ 无` 映射黑洞账户、\
                  中文币种名映射、商户名携带（精确匹配复用或即建）、金额分单位、日期格式、dedup 语义；文本内嵌 `/api/v1/openapi.json` 地址。",
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
