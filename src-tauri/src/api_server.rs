use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::Connection;

use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::AppError;
use crate::models::{Account, AccountInput, CategoryInput, CreateTransactionResult, TransactionInput};

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
        .with_state(state)
}

pub fn start_http_server(state: Arc<Mutex<Connection>>) {
    tokio::spawn(async move {
        let router = build_router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:9527")
            .await
            .expect("绑定 9527 端口失败");
        println!("Ledger API 服务已启动: http://127.0.0.1:9527");
        axum::serve(listener, router)
            .await
            .expect("HTTP 服务器异常退出");
    });
}

async fn list_accounts_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
) -> Result<Json<Vec<Account>>, AppError> {
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let accounts = query_all(
        &conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
         FROM accounts WHERE is_deleted=0 ORDER BY created_at",
        [],
    )?;
    Ok(Json(accounts))
}

async fn create_account_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    body: String,
) -> Result<(StatusCode, Json<String>), AppError> {
    let input: AccountInput =
        serde_json::from_str(&body).map_err(|e| AppError::Invalid(e.to_string()))?;
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![
            id,
            input.name,
            input.kind,
            input.currency_code,
            input.initial_balance_cents.unwrap_or(0),
            now,
            now,
            1,
            device_id()
        ],
    )?;
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
    let id = crate::commands::create_category_internal(&conn, input)?;
    Ok((StatusCode::CREATED, Json(id)))
}

async fn batch_create_transactions_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
    body: String,
) -> Result<Json<Vec<CreateTransactionResult>>, AppError> {
    let inputs: Vec<TransactionInput> =
        serde_json::from_str(&body).map_err(|e| AppError::Invalid(e.to_string()))?;
    let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute("BEGIN", [])?;
    let mut results = Vec::with_capacity(inputs.len());
    for input in inputs {
        match crate::commands::insert_transaction(&conn, input) {
            Ok(id) => results.push(CreateTransactionResult {
                success: true,
                id: Some(id),
                error: None,
            }),
            Err(AppError::Invalid(msg)) => results.push(CreateTransactionResult {
                success: false,
                id: None,
                error: Some(msg),
            }),
            Err(e) => {
                conn.execute("ROLLBACK", [])?;
                return Err(e);
            }
        }
    }
    conn.execute("COMMIT", [])?;
    Ok(Json(results))
}
