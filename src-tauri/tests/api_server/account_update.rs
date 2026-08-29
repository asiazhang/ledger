use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

use tauri_app_lib::test_utils::{CaptureLayer, ensure_global_max_level};

use crate::common::{
    batch_body, body_to_bytes, create_account_json, create_account_via_api,
    create_category_via_api, delete_account_via_api, delete_category_via_api, get_json, post_batch,
    setup_app,
};

// ---------------------------------------------------------------------------
// DELETE /api/v1/accounts/{id}
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delete_account_returns_204_and_removes_from_readback() {
    let (app, conn) = setup_app();
    let id = create_account_via_api(&app, "待删除账户").await;

    let (status, body) = delete_account_via_api(&app, &id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "204 响应应无响应体");

    let active: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, 0, "删除后该账户应 is_deleted=1");

    let (_, body) = get_json(&app, "/api/v1/accounts").await;
    let accounts = body.as_array().unwrap();
    assert!(
        !accounts.iter().any(|a| a["id"] == id),
        "删除后该账户不应出现在读回结果中"
    );
}

#[tokio::test]
async fn test_delete_account_not_found_returns_404() {
    let (app, _) = setup_app();

    let (status, body) = delete_account_via_api(&app, "不存在的id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["kind"], "NotFound");
    assert!(err["message"].as_str().unwrap().contains("账户不存在"));
}

#[tokio::test]
async fn test_delete_account_does_not_validate_references() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "有交易账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    post_batch(&app, batch_body(&[&tx], None)).await;

    let (status, _) = delete_account_via_api(&app, &account_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let tx_count: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE account_id=?1 AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tx_count, 1, "删除账户不应清理其历史交易");
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/categories/{id}
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_delete_category_returns_204_and_removes_from_readback() {
    let (app, conn) = setup_app();
    let id = create_category_via_api(&app, r#"{"name":"待删除分类","kind":"expense"}"#).await;

    let (status, body) = delete_category_via_api(&app, &id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "204 响应应无响应体");

    let active: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, 0, "删除后该分类应 is_deleted=1");

    let (_, body) = get_json(&app, "/api/v1/categories").await;
    let categories = body.as_array().unwrap();
    assert!(
        !categories.iter().any(|c| c["id"] == id),
        "删除后该分类不应出现在读回结果中"
    );
}

#[tokio::test]
async fn test_delete_category_not_found_returns_404() {
    let (app, _) = setup_app();

    let (status, body) = delete_category_via_api(&app, "不存在的id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["kind"], "NotFound");
    assert!(err["message"].as_str().unwrap().contains("分类不存在"));
}

#[tokio::test]
async fn test_delete_category_does_not_validate_references() {
    let (app, conn) = setup_app();
    let category_id =
        create_category_via_api(&app, r#"{"name":"有交易分类","kind":"expense"}"#).await;
    let account_id = create_account_via_api(&app, "现金").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","category_id":"{category_id}","date":"2026-07-01"}}"#
    );
    post_batch(&app, batch_body(&[&tx], None)).await;

    let (status, _) = delete_category_via_api(&app, &category_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let tx_count: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE category_id=?1 AND is_deleted=0",
            rusqlite::params![category_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tx_count, 1, "删除分类不应清理引用它的历史交易");
}

#[tokio::test]
async fn test_delete_account_then_reimport_recreates() {
    let (app, conn) = setup_app();
    let id = create_account_via_api(&app, "重导账户").await;

    let (status, _) = delete_account_via_api(&app, &id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 删除后重跑导入：幂等创建不再命中已删除记录，应重新创建新 id（去重位天然释放）
    let new_id = create_account_via_api(&app, "重导账户").await;
    assert_ne!(new_id, id, "删除后重导应创建新账户而非复用旧 id");
    let active: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE name=?1 AND is_deleted=0",
            rusqlite::params!["重导账户"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, 1, "重导后应恰好存在一个未删除的同名账户");
}

#[tokio::test]
async fn test_delete_category_then_reimport_recreates() {
    let (app, conn) = setup_app();
    let id = create_category_via_api(&app, r#"{"name":"重导分类","kind":"expense"}"#).await;

    let (status, _) = delete_category_via_api(&app, &id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let new_id = create_category_via_api(&app, r#"{"name":"重导分类","kind":"expense"}"#).await;
    assert_ne!(new_id, id, "删除后重导应创建新分类而非复用旧 id");
    let active: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE name=?1 AND is_deleted=0",
            rusqlite::params!["重导分类"],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, 1, "重导后应恰好存在一个未删除的同名分类");
}

// ---------------------------------------------------------------------------
// SQL 耗时归因验证（issue #44：HTTP 侧由 tower_http::trace 请求 span 归因）
// ---------------------------------------------------------------------------

/// 冒烟/回归：HTTP 导入路径的 SQL 耗时事件应归因到 `tower_http::trace` 的请求 span
/// （默认名为 `request`）。`TraceLayer::new_for_http()` 已挂载在 `build_router` 上，
/// handler 内基于 `conn.lock()` 的同步查询在请求 span 内执行，hook 事件应继承该 span。
/// 采集器具（`CaptureLayer`/`ensure_global_max_level`）来自 `tauri_app_lib::test_utils`，
/// 与单元测试 `db/tests.rs` 共用（避免重复实现）。
#[tokio::test(flavor = "current_thread")]
async fn test_http_sql_duration_attributed_to_request_span() {
    ensure_global_max_level();
    let (app, conn) = setup_app();

    // 预置账户：直接写库（在捕获 guard 之前，其 SQL 不进入断言范围），
    // 使捕获到的 SQL 只来自导入请求 span。
    let account_id = "acc-import-001";
    {
        let conn = conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'导入账户','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            rusqlite::params![account_id],
        )
        .unwrap();
    }

    let layer = CaptureLayer::new();
    let captured = Arc::clone(&layer.events);
    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

    // 把一次批量交易写入（导入路径）发到 `/api/v1/transactions/batch`。
    let batch = format!(
        r#"{{"transactions":[{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}]}}"#
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from(batch))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let events = captured.lock().unwrap().clone();
    let sql_events: Vec<_> = events
        .iter()
        .filter(|e| e.fields.iter().any(|(k, _)| k == "sql"))
        .collect();
    assert!(
        !sql_events.is_empty(),
        "应捕获到导入路径的 SQL 耗时事件，实际捕获: {events:?}"
    );
    assert!(
        sql_events
            .iter()
            .all(|e| e.current_span.as_deref() == Some("request")),
        "SQL 事件应归因到请求 span（request），实际: {sql_events:?}"
    );
}

#[tokio::test]
async fn test_put_account_renames_and_returns_updated_account() {
    let (app, _) = setup_app();

    // 创建账户
    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/accounts")
                .header("content-type", "application/json")
                .body(Body::from(create_account_json("钱包")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let id: String = serde_json::from_slice(&body_to_bytes(create_resp.into_body()).await).unwrap();

    // PUT 改名 → 200 + 更新后的完整账户
    let update_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/accounts/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"零钱"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);
    let updated: serde_json::Value =
        serde_json::from_slice(&body_to_bytes(update_resp.into_body()).await).unwrap();
    assert_eq!(updated["name"], "零钱");
    assert_eq!(updated["currency_code"], "CNY", "未传字段保持原值");

    // 读回列表应包含新名
    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/accounts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let accounts: Vec<serde_json::Value> =
        serde_json::from_slice(&body_to_bytes(list_resp.into_body()).await).unwrap();
    assert!(accounts.iter().any(|a| a["name"] == "零钱"));
}

#[tokio::test]
async fn test_put_account_returns_404_for_missing_id() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/accounts/nonexistent-id")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"任意"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
