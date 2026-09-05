//! 加密锁定门集成测试（issue #570 / ADR-0075 决策 5）：锁定期间 AI 导入
//! HTTP 面返回既有码化错误契约（`encryption.locked`），解锁后照常工作；
//! 契约自举端点（OpenAPI 文档）不受门禁影响。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, create_account_via_api, setup_app, setup_locked_app};

/// 锁定期间：数据端点（读与写）统一返回码化错误，请求不进入任何 handler——
/// 解锁前外部 AI 导入工具无法读写数据（ADR-0075 决策 5）。
#[tokio::test]
async fn locked_gate_rejects_data_endpoints_with_coded_error() {
    let app = setup_locked_app();

    // 读端点。
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

    // 写端点（AI 导入面）。
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

/// 锁定期间：契约自举端点不受门禁影响（OpenAPI 文档不含用户数据，
/// AI 工具发现契约的入口保持可用）。
#[tokio::test]
async fn locked_gate_keeps_openapi_contract_available() {
    let app = setup_locked_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// 解锁后（锁定门翻转为不锁）：同一应用数据端点照常工作——门禁只看标志，
/// 对明文库路径（标志恒 `false`）行为零变化。
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
