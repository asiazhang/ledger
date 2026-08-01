use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;
use tower_http::trace::TraceLayer;

use crate::error::AppError;
use crate::models::{
    Account, AccountInput, CategoryInput, CreateTransactionResult, Currency, TransactionBatchInput,
};

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

pub fn build_router(state: Arc<Mutex<Connection>>) -> Router {
    Router::new()
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

async fn list_accounts_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Account>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let accounts = crate::commands::list_accounts_internal(&conn)?;
    Ok(Json(accounts))
}

async fn create_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    Json(input): Json<AccountInput>,
) -> Result<(StatusCode, Json<String>), AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = crate::commands::create_account_idempotent_internal(&conn, input)?;
    Ok((StatusCode::CREATED, Json(id)))
}

async fn list_categories_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<crate::models::Category>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::commands::list_categories_internal(&conn)?))
}

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

async fn list_currencies_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Currency>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    Ok(Json(crate::commands::currencies::list_currencies_internal(
        &conn,
    )?))
}

async fn batch_create_transactions_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    Json(body): Json<TransactionBatchInput>,
) -> Result<Json<Vec<CreateTransactionResult>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let results =
        crate::commands::create_transactions_internal(&conn, body.transactions, body.dedup)?;
    Ok(Json(results))
}
