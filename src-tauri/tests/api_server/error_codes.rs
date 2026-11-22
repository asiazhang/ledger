use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use tauri_app_lib::test_support;

use crate::common::{
    body_to_bytes, create_account_via_api, create_account_via_api_with_currency,
    put_transaction_via_api, setup_app,
};

#[tokio::test]
async fn transfer_without_to_account_returns_coded_400() {
    let (app, _) = setup_app();
    let from = create_account_via_api(&app, "现金账户").await;
    let _to = create_account_via_api(&app, "银行账户").await;
    let seed = format!(
        r#"{{"transactions":[{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{from}","date":"2026-07-01"}}]}}"#
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(seed))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body_to_bytes(response.into_body()).await;
    let results: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = results[0]["id"].as_str().unwrap().to_string();

    let body = format!(
        r#"{{"kind":"transfer","amount_cents":500,"currency_code":"CNY","account_id":"{from}","date":"2026-07-01"}}"#
    );
    let (status, body) = put_transaction_via_api(&app, &id, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["kind"], "Invalid", "既有 kind 契约不变");
    assert_eq!(err["message"], "转账必须指定目标账户", "既有文案逐字不变");
    assert_eq!(err["code"], "transfer.to-account-required");
    assert!(
        err.get("params").is_none(),
        "无插值参数时 params 字段应整体缺席: {err}"
    );
}

#[tokio::test]
async fn missing_exchange_rate_returns_coded_400_with_params() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api_with_currency(&app, "现金账户", "USD").await;
    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        test_support::seed_fx_rate_history(&guard, "fxh-ec", "USD", "CNY", "2026-06-22", 7.2);
    }
    let seed = format!(
        r#"{{"transactions":[{{"kind":"expense","amount_cents":1000,"currency_code":"USD","account_id":"{account_id}","date":"2026-06-24"}}]}}"#
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(seed))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = body_to_bytes(response.into_body()).await;
    let results: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let id = results[0]["id"].as_str().unwrap().to_string();

    let body = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"USD","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let (status, body) = put_transaction_via_api(&app, &id, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(
        err["message"], "未找到 USD -> CNY 在 2026-06-29 当周的汇率：该周历史空缺",
        "按交易日入口的历史周空缺文案"
    );
    assert_eq!(err["code"], "fx.rate-missing");
    assert_eq!(
        err["params"],
        serde_json::json!(["USD", "CNY"]),
        "params 与消息中动态值顺序一致"
    );
}

#[tokio::test]
async fn update_missing_transaction_returns_coded_404() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let body = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let (status, body) = put_transaction_via_api(&app, "nonexistent-id", &body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["kind"], "NotFound", "既有 kind 契约不变");
    assert_eq!(err["message"], "交易不存在: nonexistent-id");
    assert_eq!(err["code"], "transaction.not-found");
}
