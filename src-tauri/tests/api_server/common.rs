use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use tauri_app_lib::api_server::{ApiState, FundDetailFetcher, build_router};
use tauri_app_lib::db;
use tauri_app_lib::error::AppError;
use tauri_app_lib::models::{FundDetail, FundNav};

pub(crate) async fn body_to_bytes(body: Body) -> Vec<u8> {
    body.collect().await.unwrap().to_bytes().to_vec()
}

pub(crate) fn setup_app() -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    setup_app_with_fund_fetch(None)
}

/// 装配带东财基金详情注入桩的应用（issue #304）：全部基金端点集成测试以桩
/// 离线驱动，不触真实网络；`None` 即生产路径（真实东财，测试不用）。
pub(crate) fn setup_app_with_fund_fetch(
    fund_fetch: Option<FundDetailFetcher>,
) -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    // 集成测试不经真实 Tauri 运行时，`app` 传 None（发射分支跳过，见 ApiState 注释）
    let app = build_router(ApiState {
        conn: conn.clone(),
        app: None,
        fund_fetch,
    });
    (app, conn)
}

/// 东财基金详情桩的返回形态（命中）：名称 / 东财分类 / 可选（净值，净值日期）。
pub(crate) struct FundStubHit {
    pub name: &'static str,
    pub fund_class: &'static str,
    pub nav: Option<(f64, &'static str)>,
}

/// 构造可注入的东财基金详情桩：命中表驱动（`hits` 内的代码按表返回；表外代码
/// 返回「查无此码」中文 `Invalid`——与生产 `fetch_fund_detail` 未命中同形状），
/// 并按调用顺序记录请求代码（`calls`，供测试断言「未发起网络请求」「请求了哪些
/// 代码」）。网络不可达等特殊形态由测试自建闭包或状态开关表达（先例
/// instrument_create_fund.rs 的命中/不可达切换桩）。
pub(crate) fn fund_fetch_stub(
    hits: std::collections::HashMap<String, FundStubHit>,
    calls: Arc<Mutex<Vec<String>>>,
) -> FundDetailFetcher {
    Arc::new(move |code: &str| {
        calls.lock().unwrap().push(code.to_string());
        match hits.get(code) {
            Some(hit) => Ok(FundDetail {
                code: code.to_string(),
                name: hit.name.to_string(),
                fund_class: hit.fund_class.to_string(),
                nav: hit.nav.map(|(nav, nav_date)| FundNav {
                    nav,
                    nav_date: nav_date.to_string(),
                }),
            }),
            None => Err(AppError::Invalid(format!(
                "查无基金代码 {code}，请核对后重试"
            ))),
        }
    })
}

/// 带命中表桩的一步装配（issue #304 测试便利）：返回 (router, 连接, 调用记录)，
/// 各测试按需绑定；命中表外代码由桩返回「查无此码」中文 Invalid。
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

pub(crate) fn create_account_json(account_id: &str) -> String {
    format!(
        r#"{{"name":"{}","type":"cash","currency_code":"CNY"}}"#,
        account_id
    )
}

pub(crate) async fn create_account_via_api_with_initial(
    app: &Router,
    name: &str,
    initial: i64,
) -> String {
    let body = format!(
        r#"{{"name":"{name}","type":"cash","currency_code":"CNY","initial_balance_cents":{initial}}}"#
    );
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
    let body = create_account_json(name);
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

/// POST /api/v1/instruments，返回（状态码，原始响应体）。响应体不在此处反序列化：
/// 201 为裸 id 字符串、4xx 为统一错误形状 JSON，由各测试自行解析（先例：
/// instrument_create.rs 同名辅助上收共享）。
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

/// 取响应体中的交易数组（新契约：`{items, total}` 的 `items`）。
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
