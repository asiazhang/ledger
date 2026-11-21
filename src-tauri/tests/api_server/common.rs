use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::error::AppError;
use ledger_infra::events::SignalEmitter;
use ledger_investment::prices::price_value_to_cents;
use ledger_investment::{InstrumentType, Market, Quote, StockRoute};
use tauri_app_lib::api_server::{
    ApiState, EmitterSlot, FundQuoteFetcher, QuoteFuture, StockQuoteFetcher, build_router,
};
use tauri_app_lib::test_support;

pub(crate) async fn body_to_bytes(body: Body) -> Vec<u8> {
    body.collect().await.unwrap().to_bytes().to_vec()
}

pub(crate) fn ready_quote(result: Result<Quote, AppError>) -> QuoteFuture {
    Box::pin(std::future::ready(result))
}

pub(crate) fn setup_app() -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    setup_app_with_fund_fetch(None)
}

fn build_test_app(
    fund_fetch: Option<FundQuoteFetcher>,
    stock_fetch: Option<StockQuoteFetcher>,
    emitter: EmitterSlot,
) -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    let conn = Arc::new(Mutex::new(test_support::open()));
    let app = build_router(ApiState {
        conn: conn.clone(),
        read_conn: conn.clone(),
        emitter,
        fund_fetch,
        stock_fetch,
        lock_gate: EncryptionGate::new(false),
        boot_gate: BootFailureGate::new(),
    });
    (app, conn)
}

pub(crate) fn setup_app_with_fund_fetch(
    fund_fetch: Option<FundQuoteFetcher>,
) -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    build_test_app(fund_fetch, None, None)
}

pub(crate) fn setup_app_with_stock_fetch(
    stock_fetch: Option<StockQuoteFetcher>,
) -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    build_test_app(None, stock_fetch, None)
}

pub(crate) fn setup_locked_app() -> Router {
    build_router(ApiState {
        conn: Arc::new(Mutex::new(test_support::open())),
        read_conn: Arc::new(Mutex::new(test_support::open())),
        emitter: None,
        fund_fetch: None,
        stock_fetch: None,
        lock_gate: EncryptionGate::new(true),
        boot_gate: BootFailureGate::new(),
    })
}

pub(crate) fn setup_boot_failed_app() -> Router {
    let boot_gate = BootFailureGate::new();
    boot_gate.set_failed(None);
    build_router(ApiState {
        conn: Arc::new(Mutex::new(test_support::open())),
        read_conn: Arc::new(Mutex::new(test_support::open())),
        emitter: None,
        fund_fetch: None,
        stock_fetch: None,
        lock_gate: EncryptionGate::new(false),
        boot_gate,
    })
}

pub(crate) fn setup_app_with_emitter(
    emitter: Arc<dyn SignalEmitter>,
) -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    build_test_app(None, None, Some(emitter))
}

pub(crate) struct FundStubHit {
    pub name: &'static str,
    pub fund_class: &'static str,
    pub nav: Option<(f64, &'static str)>,
    pub money_fund: bool,
}

pub(crate) fn fund_fetch_stub(
    hits: std::collections::HashMap<String, FundStubHit>,
    calls: Arc<Mutex<Vec<String>>>,
) -> FundQuoteFetcher {
    Arc::new(move |code: &str| {
        calls.lock().unwrap().push(code.to_string());
        let result = match hits.get(code) {
            Some(hit) => Ok(Quote {
                code: code.to_string(),
                name: hit.name.to_string(),
                price_cents: hit.nav.map(|(nav, _)| price_value_to_cents(nav)),
                price_date: hit.nav.map(|(_, nav_date)| nav_date.to_string()),
                market: None,
                kind_hint: None,
                fund_class: Some(hit.fund_class.to_string()),
                nav_date: hit.nav.map(|(_, nav_date)| nav_date.to_string()),
                constant_unit_price_cents: hit.money_fund.then(|| price_value_to_cents(1.0)),
                price_source: ledger_investment::prices::SINA_PRICE_SOURCE,
            }),
            None => Err(AppError::codedp(
                "sync.fund-not-found",
                format!("查无基金代码 {code}，请核对后重试"),
                &[code],
            )),
        };
        ready_quote(result)
    })
}

pub(crate) type FundStubApp = (
    Router,
    Arc<Mutex<rusqlite::Connection>>,
    Arc<Mutex<Vec<String>>>,
);

pub(crate) fn setup_app_with_fund_stub(
    hits: std::collections::HashMap<String, FundStubHit>,
) -> FundStubApp {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let fetch = fund_fetch_stub(hits, calls.clone());
    let (app, conn) = setup_app_with_fund_fetch(Some(fetch));
    (app, conn, calls)
}

pub(crate) struct StockStubHit {
    pub name: &'static str,
    pub price: Option<(i64, &'static str)>,
    pub kind_hint: InstrumentType,
    pub market: Market,
}

pub(crate) fn stock_fetch_stub(
    hits: std::collections::HashMap<String, StockStubHit>,
    calls: Arc<Mutex<Vec<(String, String)>>>,
) -> StockQuoteFetcher {
    Arc::new(move |route: StockRoute, code: &str| {
        calls
            .lock()
            .unwrap()
            .push((route.as_str().to_string(), code.to_string()));
        let result = match hits.get(&format!("{}/{code}", route.as_str())) {
            Some(hit) => Ok(Quote {
                code: code.to_string(),
                name: hit.name.to_string(),
                price_cents: hit.price.map(|(p, _)| p),
                price_date: hit.price.map(|(_, d)| d.to_string()),
                market: Some(hit.market),
                kind_hint: Some(hit.kind_hint),
                fund_class: None,
                nav_date: None,
                constant_unit_price_cents: None,
                price_source: ledger_investment::prices::TENCENT_PRICE_SOURCE,
            }),
            None => Err(AppError::codedp(
                "sync.stock-not-found",
                format!("查无股票代码 {code}，请核对后重试"),
                &[code],
            )),
        };
        ready_quote(result)
    })
}

pub(crate) type StockStubApp = (
    Router,
    Arc<Mutex<rusqlite::Connection>>,
    Arc<Mutex<Vec<(String, String)>>>,
);

pub(crate) fn setup_app_with_stock_stub(
    hits: std::collections::HashMap<String, StockStubHit>,
) -> StockStubApp {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let fetch = stock_fetch_stub(hits, calls.clone());
    let (app, conn) = setup_app_with_stock_fetch(Some(fetch));
    (app, conn, calls)
}

pub(crate) fn create_account_json(account_id: &str) -> String {
    format!(
        r#"{{"name":"{}","type":"cash","currency_code":"CNY"}}"#,
        account_id
    )
}

async fn post_account(app: &Router, body: String) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/accounts")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = body_to_bytes(response.into_body()).await;
    serde_json::from_slice(&bytes).unwrap()
}

pub(crate) async fn create_account_via_api(app: &Router, name: &str) -> String {
    post_account(app, create_account_json(name)).await
}

pub(crate) async fn create_account_via_api_with_initial(
    app: &Router,
    name: &str,
    initial: i64,
) -> String {
    post_account(
        app,
        format!(
            r#"{{"name":"{name}","type":"cash","currency_code":"CNY","initial_balance_cents":{initial}}}"#
        ),
    )
    .await
}

pub(crate) async fn create_account_via_api_with_currency(
    app: &Router,
    name: &str,
    currency: &str,
) -> String {
    post_account(
        app,
        format!(r#"{{"name":"{name}","type":"cash","currency_code":"{currency}"}}"#),
    )
    .await
}

pub(crate) async fn create_account_via_api_with_type(
    app: &Router,
    name: &str,
    account_type: &str,
) -> String {
    post_account(
        app,
        format!(r#"{{"name":"{name}","type":"{account_type}","currency_code":"CNY"}}"#),
    )
    .await
}

pub(crate) fn count_rows(conn: &rusqlite::Connection, table: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE is_deleted=0"),
        [],
        |r| r.get(0),
    )
    .unwrap()
}

pub(crate) async fn create_category_via_api(app: &Router, body: &str) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/categories")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let bytes = body_to_bytes(response.into_body()).await;
    serde_json::from_slice(&bytes).unwrap()
}

pub(crate) async fn get_first_category_id(app: &Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/categories")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body_to_bytes(response.into_body()).await;
    let categories: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    categories[0]["id"].as_str().unwrap().to_string()
}

pub(crate) async fn post_batch(app: &Router, body: String) -> Vec<serde_json::Value> {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body_to_bytes(response.into_body()).await;
    serde_json::from_slice(&bytes).unwrap()
}

pub(crate) fn batch_body(transactions: &[&str], dedup: Option<bool>) -> String {
    let tx_list = transactions.join(",");
    match dedup {
        Some(d) => format!(r#"{{"transactions":[{}],"dedup":{d}}}"#, tx_list),
        None => format!(r#"{{"transactions":[{}]}}"#, tx_list),
    }
}

pub(crate) fn count_active_transactions(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

pub(crate) async fn post_instrument(app: &Router, body: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/instruments")
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, bytes)
}

pub(crate) async fn get_json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, serde_json::from_slice(&bytes).unwrap())
}

pub(crate) async fn read_account_by_id(app: &Router, id: &str) -> serde_json::Value {
    let (status, accounts) = get_json(app, "/api/v1/accounts").await;
    assert_eq!(status, StatusCode::OK);
    accounts
        .as_array()
        .expect("账户列表应为数组")
        .iter()
        .find(|a| a["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("账户 {id} 应在列表内"))
}

pub(crate) async fn get_status(app: &Router, uri: &str) -> StatusCode {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    response.status()
}

pub(crate) fn items_of(body: &serde_json::Value) -> &[serde_json::Value] {
    body["items"]
        .as_array()
        .expect("应返回 {items, total} 结构")
        .as_slice()
}

pub(crate) async fn seed_readback_transactions(app: &Router) -> (String, String) {
    let cash = create_account_via_api(app, "现金账户").await;
    let bank = create_account_via_api(app, "银行账户").await;
    let txs = [
        format!(
            r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{cash}","date":"2026-01-01"}}"#
        ),
        format!(
            r#"{{"kind":"expense","amount_cents":200,"currency_code":"CNY","account_id":"{cash}","date":"2026-01-15"}}"#
        ),
        format!(
            r#"{{"kind":"income","amount_cents":3000,"currency_code":"CNY","account_id":"{bank}","date":"2026-02-01"}}"#
        ),
        format!(
            r#"{{"kind":"transfer","amount_cents":400,"currency_code":"CNY","account_id":"{cash}","to_account_id":"{bank}","date":"2026-02-15"}}"#
        ),
        format!(
            r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{bank}","date":"2026-03-01"}}"#
        ),
    ];
    let refs: Vec<&str> = txs.iter().map(String::as_str).collect();
    post_batch(app, batch_body(&refs, None)).await;
    (cash, bank)
}

pub(crate) fn dates_of(body: &serde_json::Value) -> Vec<&str> {
    items_of(body)
        .iter()
        .map(|t| t["date"].as_str().unwrap())
        .collect()
}

pub(crate) async fn delete_transaction_via_api(app: &Router, id: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/transactions/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, bytes)
}

pub(crate) async fn put_transaction_via_api(
    app: &Router,
    id: &str,
    body: &str,
) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/transactions/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, bytes)
}

pub(crate) fn balance_of(rows: &[serde_json::Value], name: &str) -> i64 {
    rows.iter()
        .find(|r| r["account"]["name"] == name)
        .unwrap_or_else(|| panic!("应包含账户 {name}"))["balance_cents"]
        .as_i64()
        .unwrap()
}

pub(crate) async fn delete_account_via_api(app: &Router, id: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/accounts/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, bytes)
}

pub(crate) async fn put_merchant_via_api(
    app: &Router,
    id: &str,
    body: &str,
) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/merchants/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, bytes)
}

pub(crate) async fn delete_category_via_api(app: &Router, id: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/v1/categories/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, bytes)
}
