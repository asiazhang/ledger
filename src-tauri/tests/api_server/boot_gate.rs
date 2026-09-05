//! 启动失败门集成测试（issue #601 / ADR-0075 决策 5 修订）：启动失败期间
//! AI 导入 HTTP 面返回码化错误契约（`boot.db-unreadable`），占位连接不被
//! 触达；契约自举端点（OpenAPI 文档）不受门禁影响。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, setup_boot_failed_app};

/// 启动失败期间：数据端点统一返回码化错误，请求不进入任何 handler——
/// 占位连接不是业务库，外部 AI 导入工具不得读写（与锁定期间同口径）。
#[tokio::test]
async fn boot_failed_gate_rejects_data_endpoints_with_coded_error() {
    let app = setup_boot_failed_app();

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
    assert_eq!(err["code"], "boot.db-unreadable");
    assert_eq!(
        err["message"],
        "启动失败，数据库不可用，请先在失败恢复屏重置或恢复"
    );

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
    assert_eq!(err["code"], "boot.db-unreadable");
}

/// 启动失败期间：契约自举端点不受门禁影响（OpenAPI 文档不含用户数据）。
#[tokio::test]
async fn boot_failed_gate_keeps_openapi_contract_available() {
    let app = setup_boot_failed_app();
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
