use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::{delete, get, post, put};
use rusqlite::Connection;
use tauri::AppHandle;
use tower_http::trace::TraceLayer;

use super::contract::contract_handler;
use super::handlers::accounts::{
    create_account_handler, delete_account_handler, list_account_balances_handler,
    list_accounts_handler, update_account_handler,
};
use super::handlers::categories::{
    create_category_handler, delete_category_handler, list_categories_handler,
};
use super::handlers::currencies::list_currencies_handler;
use super::handlers::funds::lookup_fund_handler;
use super::handlers::import::{import_investment_knowledge_handler, import_knowledge_handler};
use super::handlers::instruments::{create_instrument_handler, search_instruments_handler};
use super::handlers::merchants::{list_merchants_handler, update_merchant_handler};
use super::handlers::stocks::lookup_stock_handler;
use super::handlers::transactions::{
    batch_create_transactions_handler, delete_transaction_handler, list_transactions_handler,
    update_transaction_handler,
};
use super::openapi::openapi_json_handler;
use super::state::ApiState;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::error::AppError;

pub fn build_router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/openapi.json", get(openapi_json_handler))
        .route("/api/v1/contract", get(contract_handler))
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
        .route("/api/v1/stocks/{code}", get(lookup_stock_handler))
        .route("/api/v1/merchants", get(list_merchants_handler))
        .route("/api/v1/merchants/{id}", put(update_merchant_handler))
        .route("/api/v1/import/knowledge", get(import_knowledge_handler))
        .route(
            "/api/v1/import/knowledge/investment",
            get(import_investment_knowledge_handler),
        )
        .layer(TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            startup_gate_middleware,
        ))
        .with_state(state)
}

async fn startup_gate_middleware(
    State(state): State<ApiState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    const BOOTSTRAP_PATHS: [&str; 2] = ["/api/v1/openapi.json", "/api/v1/contract"];
    if !BOOTSTRAP_PATHS.contains(&req.uri().path()) {
        if state.lock_gate.is_locked() {
            return AppError::coded("encryption.locked", "应用已锁定，请先解锁后再操作")
                .into_response();
        }
        if state.boot_gate.is_failed() {
            return ledger_infra::db::boot::gate_rejection_error().into_response();
        }
    }
    next.run(req).await
}

pub fn start_http_server(
    app: AppHandle,
    state: Arc<Mutex<Connection>>,
    read_state: Arc<Mutex<Connection>>,
    lock_gate: EncryptionGate,
    boot_gate: ledger_infra::db::boot::BootFailureGate,
) {
    std::thread::spawn(move || {
        #[allow(clippy::expect_used)]
        let rt = tokio::runtime::Runtime::new().expect("创建 Tokio 运行时失败");
        rt.block_on(async move {
            let router = build_router(ApiState {
                conn: state,
                read_conn: read_state,
                emitter: Some(Arc::new(app)),
                fund_fetch: None,
                stock_fetch: None,
                lock_gate,
                boot_gate,
            });
            #[allow(clippy::expect_used)]
            let listener = tokio::net::TcpListener::bind("127.0.0.1:9527")
                .await
                .expect("绑定 9527 端口失败");
            println!("Ledger API 服务已启动: http://127.0.0.1:9527");
            #[allow(clippy::expect_used)]
            axum::serve(listener, router)
                .await
                .expect("HTTP 服务器异常退出");
        });
    });
}
