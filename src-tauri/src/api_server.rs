use std::sync::{Arc, Mutex};

use crate::error::AppError;
use crate::models::{
    Account, AccountInput, AccountType, Category, CategoryInput, CreateTransactionResult, Currency,
    TransactionBatchInput, TransactionInput,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa::ToSchema;
use utoipa_swagger_ui::SwaggerUi;

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

/// Ledger 导入 API 的 OpenAPI 契约文档。
///
/// 供 AI 编程助手读取完整端点契约（账户/分类幂等创建、批量交易去重、币种清单）。
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Ledger 导入 API",
        description = "Ledger 记账应用本地导入 API（基础地址 http://127.0.0.1:9527/api/v1）。\
                      所有金额均以整数分（`_cents` 后缀）为单位。\
                      账户/分类按自然键幂等创建；批量交易默认去重，命中返回 `duplicate: true`。",
        version = "0.1.0"
    ),
    paths(
        list_accounts_handler,
        create_account_handler,
        list_categories_handler,
        create_category_handler,
        list_currencies_handler,
        batch_create_transactions_handler
    ),
    components(schemas(
        Account,
        AccountInput,
        AccountType,
        Category,
        CategoryInput,
        Currency,
        TransactionInput,
        TransactionBatchInput,
        CreateTransactionResult,
        ErrorResponse
    ))
)]
struct ApiDoc;

pub fn build_router(state: Arc<Mutex<Connection>>) -> Router {
    Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api/v1/openapi.json", ApiDoc::openapi()))
        .layer(TraceLayer::new_for_http())
        .route(
            "/api/v1/accounts",
            get(list_accounts_handler).post(create_account_handler),
        )
        .route(
            "/api/v1/categories",
            get(list_categories_handler).post(create_category_handler),
        )
        .route(
            "/api/v1/transactions/batch",
            post(batch_create_transactions_handler),
        )
        .route("/api/v1/currencies", get(list_currencies_handler))
        .with_state(state)
}

pub fn start_http_server(state: Arc<Mutex<Connection>>) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("创建 Tokio 运行时失败");
        rt.block_on(async move {
            let router = build_router(state);
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
    Json(input): Json<AccountInput>,
) -> Result<(StatusCode, Json<String>), AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = crate::commands::create_account_idempotent_internal(&conn, input)?;
    Ok((StatusCode::CREATED, Json(id)))
}

#[utoipa::path(
    get,
    path = "/api/v1/categories",
    tag = "categories",
    summary = "列出所有分类",
    description = "返回全部分类（含种子数据），`kind` 为 `income` / `expense`，支持三级分类体系（`parent_id`）。",
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
    body: String,
) -> Result<(StatusCode, Json<String>), AppError> {
    let input: CategoryInput =
        serde_json::from_str(&body).map_err(|e| AppError::Invalid(e.to_string()))?;
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = crate::commands::create_category_idempotent_internal(&conn, input)?;
    Ok((StatusCode::CREATED, Json(id)))
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
    post,
    path = "/api/v1/transactions/batch",
    tag = "transactions",
    summary = "批量创建交易（默认去重）",
    description = "请求体为 `{ \"transactions\": TransactionInput[], \"dedup\": bool }`，`dedup` 默认 `true`。\
                  对每条交易计算确定性内容哈希 `sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`，\
                  命中已存在（`is_deleted=0`）交易则跳过并返回 `{success: true, duplicate: true, id: null}`。\
                  单条校验失败返回 `success: false` 并附带 `error`，不影响其他交易。",
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
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let results =
        crate::commands::create_transactions_internal(&conn, body.transactions, body.dedup)?;
    Ok(Json(results))
}
