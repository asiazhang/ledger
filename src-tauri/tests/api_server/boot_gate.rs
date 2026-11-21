use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, setup_boot_failed_app};

#[tokio::test]
async fn boot_failed_gate_rejects_data_endpoints_with_coded_error() {
    let app = setup_boot_failed_app();

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
    assert_eq!(err["code"], "boot.db-unreadable");
    assert_eq!(
        err["message"],
        "启动失败，数据库不可用，请先在失败恢复屏重置或恢复"
    );

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
    assert_eq!(err["code"], "boot.db-unreadable");
}

#[tokio::test]
async fn boot_failed_gate_keeps_openapi_contract_available() {
    let app = setup_boot_failed_app();
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
