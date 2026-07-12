use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use tauri_app_lib::api_server::build_router;
use tauri_app_lib::db;

async fn body_to_bytes(body: Body) -> Vec<u8> {
    body.collect().await.unwrap().to_bytes().to_vec()
}

#[tokio::test]
async fn test_get_accounts_returns_empty_list_initially() {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    let app = build_router(conn);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let accounts: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(accounts.len(), 0, "种子数据不含账户");
}

#[tokio::test]
async fn test_get_accounts_includes_newly_created_account() {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    let app = build_router(conn.clone());

    // Create an account first
    let body = r#"{"name":"现金账户","type":"cash","currency_code":"CNY"}"#;
    let create_resp = app
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
    assert_eq!(create_resp.status(), StatusCode::CREATED);

    // Now list accounts — should include the new one
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let accounts: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0]["name"], "现金账户");
}

#[tokio::test]
async fn test_create_account_returns_201() {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    let app = build_router(conn);

    let body = r#"{"name":"测试账户","type":"bank","currency_code":"CNY"}"#;
    let response = app
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
    let id: String = serde_json::from_slice(&bytes).unwrap();
    assert!(!id.is_empty(), "返回的 ID 不应为空");
}

#[tokio::test]
async fn test_create_account_with_empty_body_returns_400() {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    let app = build_router(conn);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/accounts")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let bytes = body_to_bytes(response.into_body()).await;
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
}

#[tokio::test]
async fn test_unknown_route_returns_404() {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    let app = build_router(conn);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_create_account_with_missing_fields_returns_400() {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    let app = build_router(conn);

    let body = r#"{"name":"only_name"}"#;
    let response = app
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let bytes = body_to_bytes(response.into_body()).await;
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
}
