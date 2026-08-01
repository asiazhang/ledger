use std::sync::{Arc, Mutex};

use crate::error::AppError;
use crate::models::{
    Account, AccountInput, AccountType, Category, CategoryInput, CreateTransactionResult, Currency,
    TransactionBatchInput, TransactionInput,
};
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use tower_http::trace::TraceLayer;
use utoipa::OpenApi;
use utoipa::ToSchema;

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
/// 供 AI 编程助手读取完整端点契约（账户/分类幂等创建、批量交易去重、币种清单、导入知识）。
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
        batch_create_transactions_handler,
        import_knowledge_handler
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

/// 生成式返回 OpenAPI 文档（机器可读契约，供 AI 查询端点结构）。
async fn openapi_json_handler() -> impl IntoResponse {
    Json(ApiDoc::openapi())
}

pub fn build_router(state: Arc<Mutex<Connection>>) -> Router {
    Router::new()
        .layer(TraceLayer::new_for_http())
        .route("/api/v1/openapi.json", get(openapi_json_handler))
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
        .route("/api/v1/import/knowledge", get(import_knowledge_handler))
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

/// LLM 导入知识（纯文本），供 AI 编程助手直接注入系统提示词。
///
/// 本知识是导入约定的确定性权威来源，拆解方式必须固定，否则 `dedup_hash` 漂移。
/// 端点契约（请求/响应结构）见 OpenAPI 文档。
const IMPORT_KNOWLEDGE: &str = r#"# Ledger 导入知识

## 完整契约

端点契约（请求/响应结构）见 `GET /api/v1/openapi.json`。本知识只含导入必需的确定性约定。

## 金额单位（分）

所有金额字段以「分」为单位（字段名带 `_cents` 后缀），一律用整数。
元换算成分：× 100 后取整（如 45.50 元 → `amount_cents = 4550`）。

## 币种名映射

把源数据的中文币种名映射为 `currency_code`：人民币 → `CNY`、港币 → `HKD`。
完整映射用 `GET /api/v1/currencies` 获取，勿硬编码猜测。

## 日期格式

严格使用 `YYYY-MM-DD`（如 2026-01-15），不要带时间部分。

## 交易类型判定（按金额正负）

对每一行，看 `流入金额` / `流出金额` 两列：
- `流入金额` > 0 → `kind = income`，金额取 `流入金额`
- `流出金额` > 0 → `kind = expense`，金额取 `流出金额`
- 两列同时 > 0（通常相等）→ `kind = transfer`，金额任取其一
- 两列同时为 0 → 无金额变动，无法生成合法交易（`amount_cents` 必须 > 0），跳过该行

## 转账拆分（A → B）

`资金账户` 含 ` → `（空格 + 箭头 + 空格）时拆成两个账户：
- `account_id` = 箭头左侧账户（转出方）
- `to_account_id` = 箭头右侧账户（转入方）
- `kind = transfer`，`amount_cents` 取流入/流出金额（二者相等）

## 黑洞账户（资金账户=无）

黑洞账户信号**只看 `资金账户` 列**（含 `→` 拆出的任一侧）；`收支大类=无` 不映射黑洞账户，只影响分类选择。

`资金账户` 为 `无`，或转账任一侧为 `无` 时，映射到预置黑洞账户
（`GET /api/v1/accounts` 返回，`is_hidden = true`，按币种名为 `无(CNY)` / `无(HKD)`，type 为 other）：
- 普通交易 `无` → `account_id` 指向黑洞账户，kind 照常按金额正负判定
- `x → 无` → 转账，`to_account_id` 指向黑洞账户
- `无 → x` → 转账，`account_id` 指向黑洞账户

## 幂等与去重（dedup）

- 账户/分类创建按自然键幂等：重复创建返回已有 id，不报错、不重复插入，可放心重跑。
- `POST /api/v1/transactions/batch` 默认开启去重（`dedup` 缺省 `true`）：
  - `dedup_hash = sha256(date|kind|amount_cents|currency_code|account_id|to_account_id)`
  - 字段集排除 note/category；`to_account_id` 缺省拼空串
  - 命中已存在（未删除）交易返回 `{success: true, duplicate: true, id: null}` —— 非新建也非失败，无需重试、不应上报错误
- 只匹配未删除交易：软删除后重跑会重新写入；`dedup: false` 可强制重复写入。
- `dedup_hash` 导入后保持不变，编辑备注/分类不改变它。
- 因此每行拆解必须确定性一致：同一天、同 kind、同金额、同账户的拆法若变化，哈希即漂移、去重失效。

## 备注与标签

`备注` 列写入 `note`；`标签` 列不参与交易映射，可忽略或并入 `note`。
"#;

#[utoipa::path(
    get,
    path = "/api/v1/import/knowledge",
    tag = "import",
    summary = "获取 LLM 导入知识（可直接注入系统提示词）",
    description = "返回精简的导入约定文本（text/plain），供 AI 编程助手直接注入。\
                  覆盖 Pixiu 列映射与正负判定 kind、`A → B` 转账拆分、`无`/`→ 无` 映射黑洞账户、\
                  中文币种名映射、金额分单位、日期格式、dedup 语义；文本内嵌 `/api/v1/openapi.json` 地址。",
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
