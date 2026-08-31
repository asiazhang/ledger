//! 错误码契约测试（issue #342 二期 / ADR-0049）：码化业务错误经 HTTP 序列化后
//! **只增不改**——既有 `kind`/`message` 字段取值与文案不变，新增稳定 `code`
//! 与可选 `params`（按消息中动态值出现顺序排列）。前端按码本地化，AI 导入
//! 按码自纠。此处锁定三个代表形态：400 无参（transfer 缺目标账户）、
//! 400 带参（缺汇率）、404（交易不存在）。
//!
//! 触发通道说明：单笔交易经 `POST /api/v1/transactions/batch` 是**行级容错**——
//! Invalid 类（含码化 Invalid）归入每行 `success:false`+`error` 文本，HTTP 仍 200；
//! 顶层码化错误契约由 `PUT /api/v1/transactions/{id}`（错误直接上抛）承载。

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{body_to_bytes, create_account_via_api, put_transaction_via_api, setup_app};

/// 400 码化错误（无参形态）：把既有交易改为缺 `to_account_id` 的 transfer →
/// `transfer.to-account-required`，`kind`/`message` 与既有中文逐字一致，仅新增
/// `code`；无插值参数则 `params` 字段缺席。
#[tokio::test]
async fn transfer_without_to_account_returns_coded_400() {
    let (app, _) = setup_app();
    let from = create_account_via_api(&app, "现金账户").await;
    let _to = create_account_via_api(&app, "银行账户").await;
    // 铺垫一笔可编辑的支出（batch 单行，成功路径）。
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

    // 改为 transfer 且缺 to_account_id：行为层校验失败码化，错误直接上抛（顶层 400）。
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

/// 400 码化错误（带参形态）：把既有交易币种改为无汇率的 USD → `fx.rate-missing`，
/// `params` 按消息中动态值出现顺序排列（base → quote）。
#[tokio::test]
async fn missing_exchange_rate_returns_coded_400_with_params() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let seed = format!(
        r#"{{"transactions":[{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}]}}"#
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
        err["message"], "未找到 USD -> CNY 的汇率（正反向均无）",
        "既有文案逐字不变"
    );
    assert_eq!(err["code"], "fx.rate-missing");
    assert_eq!(
        err["params"],
        serde_json::json!(["USD", "CNY"]),
        "params 与消息中动态值顺序一致"
    );
}

/// 404 码化错误：修改不存在的交易 → `transaction.not-found`，kind 保持 NotFound。
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
