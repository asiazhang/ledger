use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

use ledger_infra::test_utils::{CaptureLayer, ensure_global_max_level};
use tauri_app_lib::test_support;

use crate::common::{
    batch_body, body_to_bytes, create_account_json, create_account_via_api,
    create_category_via_api, delete_account_via_api, delete_category_via_api, get_json, post_batch,
    setup_app,
};

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
async fn test_delete_account_of_active_goal_is_rejected() {
    let (app, conn) = setup_app();
    let goal_id = ledger_savings_goal::create_savings_goal(
        &conn.lock().unwrap(),
        &ledger_savings_goal::SavingsGoalInput {
            name: "买车基金".into(),
            target_amount_cents: 500_000,
            deadline: None,
        },
    )
    .expect("创建目标应成功");
    let account_id: String = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT account_id FROM goals WHERE id=?1",
            rusqlite::params![goal_id],
            |r| r.get(0),
        )
        .unwrap();

    let (status, body) = delete_account_via_api(&app, &account_id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["code"], "savings-goal.account-in-use");

    let active: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id=?1 AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, 1, "被拒删除不应留下软删账户");
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

#[tokio::test(flavor = "current_thread")]
async fn test_http_sql_duration_attributed_to_request_span() {
    ensure_global_max_level();
    let (app, conn) = setup_app();

    let account_id = "acc-import-001";
    test_support::seed_account(
        &conn.lock().unwrap(),
        account_id,
        "导入账户",
        "cash",
        "CNY",
        0,
    );

    let layer = CaptureLayer::new();
    let captured = Arc::clone(&layer.events);
    let subscriber = tracing_subscriber::registry().with(layer);
    let _guard = tracing::subscriber::set_default(subscriber);

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

async fn create_credit_account(app: &axum::Router, name: &str, extra: &str) -> String {
    let comma = if extra.is_empty() { "" } else { "," };
    let body =
        format!(r#"{{"name":"{name}","type":"credit","currency_code":"CNY"{comma}{extra}}}"#);
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
    serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap()
}

async fn put_account(app: &axum::Router, id: &str, body: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/accounts/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = body_to_bytes(response.into_body()).await;
    (status, serde_json::from_slice(&bytes).unwrap())
}

#[tokio::test]
async fn test_put_account_sets_credit_terms() {
    let (app, _) = setup_app();
    let id = create_credit_account(&app, "招行信用卡", "").await;

    let (status, updated) = put_account(
        &app,
        &id,
        r#"{"credit_limit_cents":5000000,"statement_day":5,"due_day":25}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["credit_limit_cents"], 5_000_000);
    assert_eq!(updated["statement_day"], 5);
    assert_eq!(updated["due_day"], 25);
}

#[tokio::test]
async fn test_put_account_clears_credit_terms_with_explicit_null() {
    let (app, _) = setup_app();
    let id = create_credit_account(
        &app,
        "招行信用卡",
        r#""credit_limit_cents":5000000,"statement_day":5,"due_day":25"#,
    )
    .await;

    let (status, updated) = put_account(
        &app,
        &id,
        r#"{"credit_limit_cents":null,"statement_day":null,"due_day":null}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        updated["credit_limit_cents"].is_null(),
        "显式 null 必须清空"
    );
    assert!(updated["statement_day"].is_null());
    assert!(updated["due_day"].is_null());
}

#[tokio::test]
async fn test_put_account_without_credit_fields_keeps_them() {
    let (app, _) = setup_app();
    let id = create_credit_account(
        &app,
        "招行信用卡",
        r#""credit_limit_cents":5000000,"statement_day":5,"due_day":25"#,
    )
    .await;

    let (status, updated) = put_account(&app, &id, r#"{"name":"招行信用卡Ⅰ"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "招行信用卡Ⅰ");
    assert_eq!(updated["credit_limit_cents"], 5_000_000);
    assert_eq!(updated["statement_day"], 5);
    assert_eq!(updated["due_day"], 25);
}

#[tokio::test]
async fn test_put_account_rejects_credit_terms_for_non_credit_type() {
    let (app, _) = setup_app();
    let id = create_account_via_api(&app, "现金").await;

    let (status, err) = put_account(&app, &id, r#"{"statement_day":5}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(err["code"], "account.credit-attribute-not-applicable");

    let (_, account) = get_json(&app, "/api/v1/accounts").await;
    let cash = account
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id.as_str())
        .unwrap();
    assert!(cash["statement_day"].is_null(), "被拒绝的编辑不落库");
}

#[tokio::test]
async fn test_put_account_archives_and_restores_with_explicit_values() {
    let (app, conn) = setup_app();
    let id = create_account_via_api(&app, "借出·张三").await;

    let (status, updated) = put_account(&app, &id, r#"{"is_archived":true}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["is_archived"], true, "读回披露归档标志");
    let archived: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT is_archived FROM accounts WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(archived, 1, "归档落库");

    let (status, updated) = put_account(&app, &id, r#"{"is_archived":false}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["is_archived"], false, "恢复落定");
}

#[tokio::test]
async fn test_put_account_without_archive_key_keeps_it() {
    let (app, _) = setup_app();
    let id = create_account_via_api(&app, "借出·李四").await;
    let (status, _) = put_account(&app, &id, r#"{"is_archived":true}"#).await;
    assert_eq!(status, StatusCode::OK);

    let (status, updated) = put_account(&app, &id, r#"{"name":"借出·李四（已收回）"}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "借出·李四（已收回）");
    assert_eq!(updated["is_archived"], true, "键缺席不改归档态");
}

#[tokio::test]
async fn test_get_accounts_discloses_archive_flag() {
    let (app, _) = setup_app();
    let id = create_account_via_api(&app, "借出·王五").await;
    let (status, _) = put_account(&app, &id, r#"{"is_archived":true}"#).await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = get_json(&app, "/api/v1/accounts").await;
    let row = body
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id.as_str())
        .expect("归档账户继续出现在读面（零过滤）");
    assert_eq!(row["is_archived"], true);
}

#[tokio::test]
async fn test_put_archive_account_of_active_goal_is_rejected() {
    let (app, conn) = setup_app();
    let goal_id = ledger_savings_goal::create_savings_goal(
        &conn.lock().unwrap(),
        &ledger_savings_goal::SavingsGoalInput {
            name: "买房基金".into(),
            target_amount_cents: 100_000,
            deadline: None,
        },
    )
    .expect("创建目标应成功");
    let account_id: String = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT account_id FROM goals WHERE id=?1",
            rusqlite::params![goal_id],
            |r| r.get(0),
        )
        .unwrap();

    let (status, err) = put_account(&app, &account_id, r#"{"is_archived":true}"#).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["code"], "account.goal-bound-cannot-archive");
    assert!(
        err["message"].as_str().unwrap().contains("归档"),
        "文案应指向「归档目标本身」而不是「删除目标」"
    );

    let archived: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT is_archived FROM accounts WHERE id=?1",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(archived, 0, "被拒归档不落库");
}

fn account_op_actions(conn: &Arc<Mutex<rusqlite::Connection>>, id: &str) -> Vec<String> {
    let conn = conn.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT payload FROM sync.sync_ops ORDER BY clock")
        .unwrap();
    let payloads: Vec<serde_json::Value> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect();
    payloads
        .iter()
        .filter(|p| p["payload"]["id"] == id)
        .map(|p| {
            p["payload"]["action"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn test_put_archive_only_emits_one_op_and_repeat_is_idempotent() {
    let (app, conn) = setup_app();
    let id = create_account_via_api(&app, "借出·甲").await;
    assert_eq!(
        account_op_actions(&conn, &id),
        vec!["create"],
        "建档自身只产一条 create op"
    );

    let (status, first) = put_account(&app, &id, r#"{"is_archived":true}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["is_archived"], true, "归档落定");
    let version_archived = first["version"].as_i64().unwrap();
    assert_eq!(
        account_op_actions(&conn, &id),
        vec!["create", "set_archived"],
        "归档-only PUT 只产一条 SetArchived op（无编辑键即无 Update op）"
    );

    let (status, again) = put_account(&app, &id, r#"{"is_archived":true}"#).await;
    assert_eq!(status, StatusCode::OK, "重复归档不报错");
    assert_eq!(again["is_archived"], true);
    assert_eq!(
        again["version"].as_i64().unwrap(),
        version_archived,
        "重复归档-only PUT 不得 bump version（零 UPDATE）"
    );
    assert_eq!(
        account_op_actions(&conn, &id),
        vec!["create", "set_archived"],
        "重复归档-only PUT 不得再产 op"
    );

    let (status, restored) = put_account(&app, &id, r#"{"is_archived":false}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(restored["is_archived"], false, "恢复落定");
    let (status, again) = put_account(&app, &id, r#"{"is_archived":false}"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        again["version"].as_i64().unwrap(),
        restored["version"].as_i64().unwrap(),
        "重复恢复不得 bump version"
    );
    assert_eq!(
        account_op_actions(&conn, &id),
        vec!["create", "set_archived", "set_archived"],
        "归档 + 恢复各一条 op，重复恢复零新 op"
    );
}
