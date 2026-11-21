use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, create_account_via_api, setup_app, setup_locked_app};

#[tokio::test]
async fn locked_gate_rejects_data_endpoints_with_coded_error() {
    let app = setup_locked_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let err: serde_json::Value =
        serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();
    assert_eq!(err["kind"], "Invalid", "码化错误归类契约（ADR-0050）");
    assert_eq!(err["code"], "encryption.locked");
    assert_eq!(err["message"], "应用已锁定，请先解锁后再操作");

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"transactions":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let err: serde_json::Value =
        serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();
    assert_eq!(err["code"], "encryption.locked");
}

#[tokio::test]
async fn locked_gate_keeps_openapi_contract_available() {
    let app = setup_locked_app();
    for uri in ["/api/v1/openapi.json", "/api/v1/contract"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{uri} 应不受门禁影响");
    }
}

#[tokio::test]
async fn unlocked_gate_serves_requests_normally() {
    let (app, _) = setup_app();
    let _ = create_account_via_api(&app, "现金账户").await;
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let accounts: serde_json::Value =
        serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();
    let list = accounts.as_array().unwrap();
    assert!(
        list.iter().any(|a| a["name"] == "现金账户"),
        "解锁后数据端点照常工作，新建账户应可读: {list:?}"
    );
}
