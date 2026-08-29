use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use tauri_app_lib::api_server::{ApiState, build_router};
use tauri_app_lib::db;

pub(crate) async fn body_to_bytes(body: Body) -> Vec<u8> {
    body.collect().await.unwrap().to_bytes().to_vec()
}

pub(crate) fn setup_app() -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    // 集成测试不经真实 Tauri 运行时，`app` 传 None（发射分支跳过，见 ApiState 注释）
    let app = build_router(ApiState {
        conn: conn.clone(),
        app: None,
    });
    (app, conn)
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
