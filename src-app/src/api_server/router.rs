//! 路由表与 HTTP 服务器启动：端点注册单点（与 OpenAPI 契约同源维护）。

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::routing::{delete, get, post, put};
use rusqlite::Connection;
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
use super::state::{ApiState, EmitterSlot};
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

/// 启动门中间件（加密锁定门 + 启动失败门，issue #570 / #601）：除契约
/// 自举端点（标准 OpenAPI 与紧凑方言，issue #839）外的全部端点在锁定期间
/// 返回码化错误 `encryption.locked`、在启动失败期间返回码化错误
/// `boot.db-unreadable`——AI 导入 HTTP 面在解锁前/库不可用时不可用；标志
/// 翻转（IPC 壳命令统一置位）后请求照常放行，契约面零变化。
async fn startup_gate_middleware(
    State(state): State<ApiState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    // 契约自举端点：不含用户数据、不触 DB，锁定/启动失败期间照常可用。
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

/// 实际绑定的端口（#2112 端口去固定化的可观察面）：`Some` = 本进程 HTTP
/// 服务已绑定成功；`None` = 尚未绑定（含后台线程尚未完成建连的窗口）。
/// headless 接线断言经 [`super::super::http_host::mounted_port`] 消费同一事实。
static HTTP_BOUND_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

/// 实际绑定端口读取口（headless 测试消费，生产零依赖）。
#[doc(hidden)]
pub fn bound_port() -> Option<u16> {
    HTTP_BOUND_PORT.get().copied()
}

/// 绑定监听：默认端口 `127.0.0.1:9527`；被占用（同机并发测试进程 / 第二实
/// 例）时回落内核分配的临时端口（#2112 端口去固定化）——产品默认端口与
/// fail loud 语义不变（非占用类绑定错误照旧 fatal），只是「端口冲突」不再
/// 是启动故障：冲突方各得一个可用服务，互不劫持对方的测试/会话现场。
async fn bind_listener() -> std::io::Result<tokio::net::TcpListener> {
    match tokio::net::TcpListener::bind("127.0.0.1:9527").await {
        Ok(listener) => Ok(listener),
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            println!("9527 端口被占用，回落到内核分配的临时端口（#2112）");
            tokio::net::TcpListener::bind("127.0.0.1:0").await
        }
        Err(err) => Err(err),
    }
}

pub fn start_http_server(
    emitter: EmitterSlot,
    state: Arc<Mutex<Connection>>,
    read_state: Arc<Mutex<Connection>>,
    lock_gate: EncryptionGate,
    boot_gate: ledger_infra::db::boot::BootFailureGate,
) {
    std::thread::spawn(move || {
        // B 类豁免（ADR-0060）：HTTP 壳启动期创建 Tokio 运行时，失败即无法运行。
        #[allow(clippy::expect_used)]
        let rt = tokio::runtime::Runtime::new().expect("创建 Tokio 运行时失败");
        rt.block_on(async move {
            let router = build_router(ApiState {
                conn: state,
                read_conn: read_state,
                // 发射器接缝（ADR-0054 #367 修订泛化）：调用方注入生产发射器
                //（NSH 泵发射器，ADR-0112 决策 5 壳层接线）；None 跳过发射分支。
                emitter,
                fund_fetch: None,
                stock_fetch: None,
                lock_gate,
                boot_gate,
            });
            // B 类豁免（ADR-0060）：启动期绑定失败即无法运行（占用类错误已
            // 在 bind_listener 回落临时端口，#2112）。
            #[allow(clippy::expect_used)]
            let listener = bind_listener().await.expect("绑定 9527 端口失败");
            let addr = listener.local_addr();
            let port = {
                // B 类豁免（ADR-0060）：绑定成功后读本地址，失败即启动期致命。
                #[allow(clippy::expect_used)]
                addr.expect("读取绑定端口失败").port()
            };
            HTTP_BOUND_PORT.get_or_init(|| port);
            println!("Ledger API 服务已启动: http://127.0.0.1:{port}");
            // B 类豁免（ADR-0060）：服务器异常退出属启动期致命故障，fail loud。
            #[allow(clippy::expect_used)]
            axum::serve(listener, router)
                .await
                .expect("HTTP 服务器异常退出");
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 端口去固定化契约（#2112）：默认端口被占用（同机并发测试进程 / 第二
    /// 实例）时回落内核分配的临时端口，服务本体不受损；实际绑定端口经
    /// [`bound_port`] 可观察。删除回落分支本测超时红——并发跑 `cargo test
    /// -p nsh-app` 时第二个进程将永久绑不上默认端口。
    #[test]
    fn binds_ephemeral_port_when_default_is_occupied() {
        // 占位默认端口：同机并发测试进程 / 第二实例已在占用时佔位失败——
        // 占用前提天然成立，直接进入被测路径（#2112 现场即多进程并发跑）。
        let _squatter = std::net::TcpListener::bind("127.0.0.1:9527").ok();
        let state = Arc::new(Mutex::new(crate::test_support::open()));
        let read_state = Arc::new(Mutex::new(crate::test_support::open()));
        start_http_server(
            None,
            state,
            read_state,
            EncryptionGate::new(false),
            ledger_infra::db::boot::BootFailureGate::new(),
        );
        let mut bound = None;
        for _ in 0..50 {
            if let Some(port) = bound_port() {
                bound = Some(port);
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let port = bound.expect("默认端口被占时应回落临时端口并报告绑定结果");
        assert_ne!(port, 9527, "回落端口不得是仍被占用的默认端口");
    }
}
