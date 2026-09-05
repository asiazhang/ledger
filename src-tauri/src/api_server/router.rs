//! 路由表与 HTTP 服务器启动：端点注册单点（与 OpenAPI 契约同源维护）。

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::{delete, get, post, put};
use rusqlite::Connection;
use tauri::AppHandle;
use tower_http::trace::TraceLayer;

use super::handlers::accounts::{
    create_account_handler, delete_account_handler, list_account_balances_handler,
    list_accounts_handler, update_account_handler,
};
use super::handlers::categories::{
    create_category_handler, delete_category_handler, list_categories_handler,
};
use super::handlers::currencies::list_currencies_handler;
use super::handlers::funds::lookup_fund_handler;
use super::handlers::import::import_knowledge_handler;
use super::handlers::instruments::{create_instrument_handler, search_instruments_handler};
use super::handlers::merchants::list_merchants_handler;
use super::handlers::transactions::{
    batch_create_transactions_handler, delete_transaction_handler, list_transactions_handler,
    update_transaction_handler,
};
use super::openapi::openapi_json_handler;
use super::state::ApiState;
use crate::db::encryption::EncryptionGate;
use crate::error::AppError;
use axum::extract::State;
use axum::http::Request;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};

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
        // 加密锁定门（issue #570 / ADR-0075 决策 5）：置于最外层，锁定期间
        // 除契约自举端点外一律返回码化错误，请求不进入任何 handler。
        .layer(middleware::from_fn_with_state(
            state.clone(),
            startup_gate_middleware,
        ))
        .with_state(state)
}

/// 启动门中间件（加密锁定门 + 启动失败门，issue #570 / #601）：除 OpenAPI
/// 契约自举端点外的全部端点在锁定期间返回码化错误 `encryption.locked`、
/// 在启动失败期间返回码化错误 `boot.db-unreadable`——AI 导入 HTTP 面在解锁
/// 前/库不可用时不可用；标志翻转（IPC 壳命令统一置位）后请求照常放行，
/// 契约面零变化。
async fn startup_gate_middleware(
    State(state): State<ApiState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    const OPENAPI_PATH: &str = "/api/v1/openapi.json";
    if req.uri().path() != OPENAPI_PATH {
        if state.lock_gate.is_locked() {
            return AppError::coded("encryption.locked", "应用已锁定，请先解锁后再操作")
                .into_response();
        }
        if state.boot_gate.is_failed() {
            return crate::db::boot::gate_rejection_error().into_response();
        }
    }
    next.run(req).await
}

pub fn start_http_server(
    app: AppHandle,
    state: Arc<Mutex<Connection>>,
    lock_gate: EncryptionGate,
    boot_gate: crate::db::boot::BootFailureGate,
) {
    std::thread::spawn(move || {
        // B 类豁免（ADR-0060）：HTTP 壳启动期创建 Tokio 运行时，失败即无法运行。
        #[allow(clippy::expect_used)]
        let rt = tokio::runtime::Runtime::new().expect("创建 Tokio 运行时失败");
        rt.block_on(async move {
            let router = build_router(ApiState {
                conn: state,
                emitter: Some(Arc::new(app)),
                fund_fetch: None,
                lock_gate,
                boot_gate,
            });
            // B 类豁免（ADR-0060）：启动期绑定 9527 端口失败即无法运行。
            #[allow(clippy::expect_used)]
            let listener = tokio::net::TcpListener::bind("127.0.0.1:9527")
                .await
                .expect("绑定 9527 端口失败");
            println!("Ledger API 服务已启动: http://127.0.0.1:9527");
            // B 类豁免（ADR-0060）：服务器异常退出属启动期致命故障，fail loud。
            #[allow(clippy::expect_used)]
            axum::serve(listener, router)
                .await
                .expect("HTTP 服务器异常退出");
        });
    });
}
