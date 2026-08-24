use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use tracing_subscriber::layer::SubscriberExt;

use tauri_app_lib::api_server::build_router;
use tauri_app_lib::db;
use tauri_app_lib::test_utils::{CaptureLayer, ensure_global_max_level};

async fn body_to_bytes(body: Body) -> Vec<u8> {
    body.collect().await.unwrap().to_bytes().to_vec()
}

fn setup_app() -> (Router, Arc<Mutex<rusqlite::Connection>>) {
    let mut conn = db::open_in_memory().unwrap();
    db::init_db(&mut conn).unwrap();
    let conn = Arc::new(Mutex::new(conn));
    let app = build_router(conn.clone());
    (app, conn)
}

fn create_account_json(account_id: &str) -> String {
    format!(
        r#"{{"name":"{}","type":"cash","currency_code":"CNY"}}"#,
        account_id
    )
}

#[tokio::test]
async fn test_get_accounts_returns_black_hole_seed_accounts_initially() {
    let (app, _) = setup_app();

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
    assert_eq!(accounts.len(), 2, "种子应预置两个黑洞账户");
    for a in &accounts {
        assert_eq!(a["is_hidden"], true, "种子黑洞账户应带 is_hidden=true");
        assert_eq!(a["type"], "other");
    }
    assert!(accounts.iter().any(|a| a["name"] == "无(CNY)"));
    assert!(accounts.iter().any(|a| a["name"] == "无(HKD)"));
}

#[tokio::test]
async fn test_get_accounts_includes_newly_created_account() {
    let (app, _) = setup_app();

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/accounts")
                .header("content-type", "application/json")
                .body(Body::from(create_account_json("现金账户")))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::CREATED);

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
    assert_eq!(accounts.len(), 3);
    let created = accounts
        .iter()
        .find(|a| a["name"] == "现金账户")
        .expect("应包含新建账户");
    assert_eq!(created["is_hidden"], false);
}

#[tokio::test]
async fn test_create_account_returns_201() {
    let (app, _) = setup_app();

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
async fn test_create_account_with_empty_body_returns_422() {
    let (app, _) = setup_app();

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

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_unknown_route_returns_404() {
    let (app, _) = setup_app();

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
async fn test_create_account_with_missing_fields_returns_422() {
    let (app, _) = setup_app();

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

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn test_get_categories_returns_seed_data() {
    let (app, _) = setup_app();

    let response = app
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
    assert!(categories.len() >= 92, "种子数据应包含 92 个分类");
    let expense_count = categories.iter().filter(|c| c["kind"] == "expense").count();
    let income_count = categories.iter().filter(|c| c["kind"] == "income").count();
    assert!(expense_count > 0);
    assert!(income_count > 0);
}

#[tokio::test]
async fn test_create_category_returns_201() {
    let (app, _) = setup_app();

    let body = r#"{"name":"交通出行","kind":"expense"}"#;
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/categories")
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
async fn test_create_category_with_empty_body_returns_400() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/categories")
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

async fn create_account_via_api_with_initial(app: &Router, name: &str, initial: i64) -> String {
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

async fn create_account_via_api(app: &Router, name: &str) -> String {
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

fn count_rows(conn: &rusqlite::Connection, table: &str) -> i64 {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table} WHERE is_deleted=0"),
        [],
        |r| r.get(0),
    )
    .unwrap()
}

async fn create_category_via_api(app: &Router, body: &str) -> String {
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

async fn get_first_category_id(app: &Router) -> String {
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

#[tokio::test]
async fn test_create_account_idempotent_returns_same_id() {
    let (app, conn) = setup_app();

    let first = create_account_via_api(&app, "现金账户").await;
    let second = create_account_via_api(&app, "现金账户").await;

    assert_eq!(first, second, "同名账户应返回同一 id");
    let visible: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_deleted=0 AND is_hidden=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(visible, 1, "用户侧应仅有一行（黑洞账户不计入）");
}

#[tokio::test]
async fn test_create_account_idempotent_distinguishes_currency() {
    let (app, _) = setup_app();

    let cny = create_account_via_api(&app, "多币种账户").await;
    let body = r#"{"name":"多币种账户","type":"cash","currency_code":"HKD"}"#;
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
    let hkd: String = serde_json::from_slice(&bytes).unwrap();

    assert_ne!(cny, hkd, "不同币种应视为不同账户");
}

#[tokio::test]
async fn test_create_account_idempotent_distinguishes_type() {
    let (app, _) = setup_app();

    let cash = create_account_via_api(&app, "类型账户").await;
    let body = r#"{"name":"类型账户","type":"bank","currency_code":"CNY"}"#;
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
    let bank: String = serde_json::from_slice(&bytes).unwrap();

    assert_ne!(cash, bank, "不同类型应视为不同账户");
}

#[tokio::test]
async fn test_create_category_idempotent_returns_same_id() {
    let (app, conn) = setup_app();
    let baseline = count_rows(&conn.lock().unwrap(), "categories");

    let body = r#"{"name":"交通出行","kind":"expense"}"#;
    let first = create_category_via_api(&app, body).await;
    let second = create_category_via_api(&app, body).await;

    assert_eq!(first, second, "同名同类型分类应返回同一 id");
    assert_eq!(
        count_rows(&conn.lock().unwrap(), "categories"),
        baseline + 1,
        "库中应仅新增一行"
    );
}

#[tokio::test]
async fn test_create_category_idempotent_distinguishes_kind() {
    let (app, _) = setup_app();

    let exp_id = create_category_via_api(&app, r#"{"name":"测试分类","kind":"expense"}"#).await;
    let inc_id = create_category_via_api(&app, r#"{"name":"测试分类","kind":"income"}"#).await;

    assert_ne!(exp_id, inc_id, "不同类型应视为不同分类");
}

#[tokio::test]
async fn test_create_category_idempotent_distinguishes_parent() {
    let (app, conn) = setup_app();
    let baseline = count_rows(&conn.lock().unwrap(), "categories");

    let parent_a = create_category_via_api(&app, r#"{"name":"父分类A","kind":"expense"}"#).await;
    let parent_b = create_category_via_api(&app, r#"{"name":"父分类B","kind":"expense"}"#).await;

    let child_a1 = create_category_via_api(
        &app,
        &format!(r#"{{"name":"子分类","kind":"expense","parent_id":"{parent_a}"}}"#),
    )
    .await;
    let child_a2 = create_category_via_api(
        &app,
        &format!(r#"{{"name":"子分类","kind":"expense","parent_id":"{parent_a}"}}"#),
    )
    .await;
    let child_b = create_category_via_api(
        &app,
        &format!(r#"{{"name":"子分类","kind":"expense","parent_id":"{parent_b}"}}"#),
    )
    .await;

    assert_eq!(child_a1, child_a2, "同父分类同名应返回同一 id");
    assert_ne!(child_a1, child_b, "不同父分类应视为不同分类");
    assert_eq!(
        count_rows(&conn.lock().unwrap(), "categories"),
        baseline + 4,
        "库中应仅新增 4 行"
    );
}

#[tokio::test]
async fn test_get_currencies_returns_seed_list() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/currencies")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let currencies: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert!(currencies.len() >= 11, "应返回全部种子币种");
    for c in &currencies {
        assert!(c["code"].is_string());
        assert!(c["name"].is_string());
        assert!(c["symbol"].is_string());
        assert!(c["decimal_places"].is_number());
    }
    let cny = currencies.iter().find(|c| c["code"] == "CNY").unwrap();
    assert_eq!(cny["name"], "人民币");
    assert_eq!(cny["decimal_places"], 2);
    let hkd = currencies.iter().find(|c| c["code"] == "HKD").unwrap();
    assert_eq!(hkd["name"], "港币");
}

#[tokio::test]
async fn test_batch_create_transactions_all_success() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let body = format!(
        r#"{{
            "transactions": [
                {{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}},
                {{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}},
                {{"kind":"income","amount_cents":2000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}}
            ]
        }}"#
    );

    let response = app
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
    let results: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.len(), 3);
    for r in &results {
        assert_eq!(r["success"], true);
        assert_eq!(r["duplicate"], false);
        assert!(!r["id"].as_str().unwrap_or("").is_empty());
    }
}

#[tokio::test]
async fn test_batch_create_transactions_partial_failure() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let body = format!(
        r#"{{
            "transactions": [
                {{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}},
                {{"kind":"income","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}},
                {{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}},
                {{"kind":"transfer","amount_cents":300,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-04"}}
            ]
        }}"#
    );

    let response = app
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
    let results: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(results.len(), 4);
    assert_eq!(results[0]["success"], true);
    assert_eq!(results[0]["duplicate"], false);
    assert_eq!(results[1]["success"], false);
    assert!(results[1]["error"].as_str().unwrap().contains("大于 0"));
    assert_eq!(results[2]["success"], true);
    assert_eq!(results[3]["success"], false);
    assert!(results[3]["error"].as_str().unwrap().contains("目标账户"));

    let count: i64 = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_batch_create_transactions_invalid_json_returns_400() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/transactions/batch")
                .header("content-type", "application/json")
                .body(Body::from("not-json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

async fn post_batch(app: &Router, body: String) -> Vec<serde_json::Value> {
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

fn batch_body(transactions: &[&str], dedup: Option<bool>) -> String {
    let tx_list = transactions.join(",");
    match dedup {
        Some(d) => format!(r#"{{"transactions":[{}],"dedup":{d}}}"#, tx_list),
        None => format!(r#"{{"transactions":[{}]}}"#, tx_list),
    }
}

fn count_active_transactions(conn: &rusqlite::Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[tokio::test]
async fn test_batch_same_batch_twice_marks_all_duplicates_and_keeps_row_count() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx1 = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let tx2 = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );

    let first = post_batch(&app, batch_body(&[&tx1, &tx2], None)).await;
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|r| r["success"] == true && r["duplicate"] == false)
    );

    let second = post_batch(&app, batch_body(&[&tx1, &tx2], None)).await;
    assert_eq!(second.len(), 2);
    assert!(
        second
            .iter()
            .all(|r| r["success"] == true && r["duplicate"] == true && r["id"].is_null()),
        "第二次导入应全部命中重复"
    );

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 2, "重复导入不应增加库中行数");
}

#[tokio::test]
async fn test_batch_dedup_false_writes_duplicates() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );

    let first = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(first[0]["duplicate"], false);

    let second = post_batch(&app, batch_body(&[&tx], Some(false))).await;
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["duplicate"], false);
    assert!(
        !second[0]["id"].as_str().unwrap_or("").is_empty(),
        "dedup=false 应重复写入"
    );

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 2);
}

#[tokio::test]
async fn test_batch_dedup_ignores_note_and_category_change() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let category_id = get_first_category_id(&app).await;

    let base = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );
    let with_note = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02","note":"改了备注"}}"#
    );
    let with_category = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02","category_id":"{category_id}"}}"#
    );

    post_batch(&app, batch_body(&[&base], None)).await;
    let second = post_batch(&app, batch_body(&[&with_note], None)).await;
    assert_eq!(second[0]["duplicate"], true, "仅改备注应命中重复");
    let third = post_batch(&app, batch_body(&[&with_category], None)).await;
    assert_eq!(third[0]["duplicate"], true, "仅改分类应命中重复");

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_batch_dedup_not_hit_when_amount_account_date_change() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let other_account_id = create_account_via_api(&app, "另一账户").await;

    let base = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );
    let diff_amount = format!(
        r#"{{"kind":"expense","amount_cents":600,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );
    let diff_account = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{other_account_id}","date":"2026-07-02"}}"#
    );
    let diff_date = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}}"#
    );

    post_batch(&app, batch_body(&[&base], None)).await;

    for changed in [&diff_amount, &diff_account, &diff_date] {
        let result = post_batch(&app, batch_body(&[changed], None)).await;
        assert_eq!(
            result[0]["duplicate"], false,
            "改金额/账户/日期不应命中重复"
        );
        assert!(!result[0]["id"].as_str().unwrap_or("").is_empty());
    }
}

#[tokio::test]
async fn test_batch_dedup_soft_deleted_then_reimport_writes_again() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );

    let first = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = first[0]["id"].as_str().unwrap().to_string();

    {
        let conn = conn.lock().unwrap();
        conn.execute(
            "UPDATE transactions SET is_deleted=1 WHERE id=?1",
            rusqlite::params![id],
        )
        .unwrap();
    }

    let second = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(second[0]["duplicate"], false, "软删除后重跑应重新写入");
    assert!(!second[0]["id"].as_str().unwrap_or("").is_empty());

    let count: i64 = {
        let conn = conn.lock().unwrap();
        count_active_transactions(&conn)
    };
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_batch_dedup_keeps_dedup_hash_unchanged() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );

    post_batch(&app, batch_body(&[&tx], None)).await;

    let hash: Option<String> = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT dedup_hash FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert!(hash.is_some(), "导入后应写入 dedup_hash");

    // 重复导入命中重复后，dedup_hash 保持原值不变（编辑/同步无特殊处理）
    let second = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(second[0]["duplicate"], true);
    let hash_after: Option<String> = {
        let conn = conn.lock().unwrap();
        conn.query_row(
            "SELECT dedup_hash FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(hash, hash_after, "dedup_hash 导入后保持不变");
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, serde_json::Value) {
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
fn items_of(body: &serde_json::Value) -> &[serde_json::Value] {
    body["items"]
        .as_array()
        .expect("应返回 {items, total} 结构")
        .as_slice()
}

#[tokio::test]
async fn test_get_transactions_returns_empty_list_when_none() {
    let (app, _) = setup_app();

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    assert!(items_of(&body).is_empty(), "无交易时应返回空 items");
    assert_eq!(body["total"], 0, "total 应为 0");
}

#[tokio::test]
async fn test_get_transactions_returns_all_undeleted_newest_first() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let tx_jan = format!(
        r#"{{"kind":"income","amount_cents":100,"currency_code":"CNY","account_id":"{account_id}","date":"2026-01-01"}}"#
    );
    let tx_mar = format!(
        r#"{{"kind":"expense","amount_cents":300,"currency_code":"CNY","account_id":"{account_id}","date":"2026-03-01"}}"#
    );
    let tx_feb = format!(
        r#"{{"kind":"income","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-02-01"}}"#
    );
    post_batch(&app, batch_body(&[&tx_jan, &tx_mar, &tx_feb], None)).await;

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 3);
    assert_eq!(body["total"], 3, "缺省返回全部时 total 应为全部条数");
    assert_eq!(txs[0]["date"], "2026-03-01");
    assert_eq!(txs[0]["amount_cents"], 300);
    assert_eq!(txs[1]["date"], "2026-02-01");
    assert_eq!(txs[1]["amount_cents"], 200);
    assert_eq!(txs[2]["date"], "2026-01-01");
    assert_eq!(txs[2]["amount_cents"], 100);
    for tx in txs {
        assert_eq!(tx["is_deleted"], false);
        assert_eq!(tx["account_id"], account_id);
    }
}

async fn seed_readback_transactions(app: &Router) -> (String, String) {
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

fn dates_of(body: &serde_json::Value) -> Vec<&str> {
    items_of(body)
        .iter()
        .map(|t| t["date"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn test_get_transactions_from_to_is_inclusive() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?from=2026-01-15&to=2026-02-15").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        dates_of(&body),
        vec!["2026-02-15", "2026-02-01", "2026-01-15"]
    );
    assert_eq!(body["total"], 3, "日期过滤后 total 应为过滤后总数");
    let sum: i64 = items_of(&body)
        .iter()
        .map(|t| t["amount_cents"].as_i64().unwrap())
        .sum();
    assert_eq!(sum, 3600);
}

#[tokio::test]
async fn test_get_transactions_filters_by_account_id() {
    let (app, _) = setup_app();
    let (cash, bank) = seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, &format!("/api/v1/transactions?account_id={cash}")).await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 3);
    assert_eq!(body["total"], 3);
    assert!(txs.iter().all(|t| t["account_id"] == cash));
    assert!(txs.iter().all(|t| t["account_id"] != bank));
    let sum: i64 = txs
        .iter()
        .map(|t| t["amount_cents"].as_i64().unwrap())
        .sum();
    assert_eq!(sum, 1600);
}

#[tokio::test]
async fn test_get_transactions_filters_by_kind() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?kind=expense").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2);
    assert_eq!(body["total"], 2);
    assert!(txs.iter().all(|t| t["kind"] == "expense"));
    let sum: i64 = txs
        .iter()
        .map(|t| t["amount_cents"].as_i64().unwrap())
        .sum();
    assert_eq!(sum, 700);
}

#[tokio::test]
async fn test_get_transactions_limit_truncates() {
    let (app, _) = setup_app();
    seed_readback_transactions(&app).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?limit=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dates_of(&body), vec!["2026-03-01", "2026-02-15"]);
    assert_eq!(body["total"], 5, "limit 只截取 items，total 仍为过滤后总数");
}

#[tokio::test]
async fn test_get_transactions_pagination_returns_page_and_total() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let txs: Vec<String> = (1..=25)
        .map(|i| {
            format!(
                r#"{{"kind":"expense","amount_cents":{},"currency_code":"CNY","account_id":"{account_id}","date":"2026-06-{:02}"}}"#,
                i * 100,
                i
            )
        })
        .collect();
    let refs: Vec<&str> = txs.iter().map(String::as_str).collect();
    post_batch(&app, batch_body(&refs, None)).await;

    let (status, p1) = get_json(&app, "/api/v1/transactions?page=1&page_size=10").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(items_of(&p1).len(), 10, "第 1 页应返回 10 条");
    assert_eq!(p1["total"], 25);

    let (_, p3) = get_json(&app, "/api/v1/transactions?page=3&page_size=10").await;
    assert_eq!(items_of(&p3).len(), 5, "第 3 页应返回剩余 5 条");
    assert_eq!(p3["total"], 25);
}

#[tokio::test]
async fn test_get_transactions_pagination_out_of_range_page() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":300,"currency_code":"CNY","account_id":"{account_id}","date":"2026-06-01"}}"#
    );
    post_batch(&app, batch_body(&[&tx], None)).await;

    let (status, body) = get_json(&app, "/api/v1/transactions?page=99&page_size=10").await;
    assert_eq!(status, StatusCode::OK);
    assert!(items_of(&body).is_empty(), "超范围页码应返回空 items");
    assert_eq!(body["total"], 1, "total 仍为过滤后总数");
}

#[tokio::test]
async fn test_get_transactions_excludes_soft_deleted() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let keep = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-01-01"}}"#
    );
    let drop = format!(
        r#"{{"kind":"expense","amount_cents":200,"currency_code":"CNY","account_id":"{account_id}","date":"2026-01-02"}}"#
    );
    let created = post_batch(&app, batch_body(&[&keep, &drop], None)).await;
    let deleted_id = created[1]["id"].as_str().unwrap();
    {
        let conn = conn.lock().unwrap();
        conn.execute(
            "UPDATE transactions SET is_deleted=1 WHERE id=?1",
            rusqlite::params![deleted_id],
        )
        .unwrap();
    }

    let (status, body) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(status, StatusCode::OK);
    let txs = items_of(&body);
    assert_eq!(txs.len(), 1);
    assert_eq!(body["total"], 1);
    assert_eq!(txs[0]["amount_cents"], 1000);
    assert_eq!(txs[0]["date"], "2026-01-01");
}

async fn delete_transaction_via_api(app: &Router, id: &str) -> (StatusCode, Vec<u8>) {
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

#[tokio::test]
async fn test_delete_transaction_returns_204_and_removes_from_readback() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap();

    let (status, body) = delete_transaction_via_api(&app, id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_empty(), "204 响应应无响应体");

    let active: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(active, 0, "删除后该交易应 is_deleted=1");

    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&body);
    assert!(
        !txs.iter().any(|t| t["id"] == id),
        "删除后该行不应出现在读回结果中"
    );
}

#[tokio::test]
async fn test_delete_transaction_not_found_returns_404() {
    let (app, _) = setup_app();

    let (status, body) = delete_transaction_via_api(&app, "不存在的id").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let err: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(err["kind"], "NotFound");
    assert!(err["message"].as_str().unwrap().contains("交易不存在"));
}

#[tokio::test]
async fn test_delete_transaction_frees_dedup_slot_for_reimport() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );

    let first = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = first[0]["id"].as_str().unwrap();

    let (status, _) = delete_transaction_via_api(&app, id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let second = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(second[0]["duplicate"], false, "删除后重跑应重新写入");
    assert!(!second[0]["id"].as_str().unwrap_or("").is_empty());
}

async fn put_transaction_via_api(app: &Router, id: &str, body: &str) -> (StatusCode, Vec<u8>) {
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

#[tokio::test]
async fn test_update_transaction_returns_200_and_updates_fields() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap();

    let body = format!(
        r#"{{"kind":"expense","amount_cents":900,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-10","note":"改后"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, id, &body).await;
    assert_eq!(status, StatusCode::OK, "PUT 应返回 200");
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["amount_cents"], 900);
    assert_eq!(updated["date"], "2026-07-10");
    assert_eq!(updated["note"], "改后");
    assert_eq!(updated["id"], id, "应保持同一 id");
    assert_eq!(updated["version"], 2, "修改后版本号应递增");

    // 读回应反映修改。
    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&readback);
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0]["amount_cents"], 900);
    assert_eq!(txs[0]["note"], "改后");
}

#[tokio::test]
async fn test_update_transaction_not_found_returns_404() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;

    let body = format!(
        r#"{{"kind":"expense","amount_cents":100,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, "不存在的id", &body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "NotFound");
    assert!(err["message"].as_str().unwrap().contains("交易不存在"));
}

#[tokio::test]
async fn test_update_transaction_reuses_kind_validation_returns_400() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap();

    // 改成转账但缺目标账户，应与创建路径一致返回 Invalid。
    let body = format!(
        r#"{{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-02"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, id, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "缺目标账户应返回 400");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert!(err["message"].as_str().unwrap().contains("目标账户"));
}

#[tokio::test]
async fn test_update_transaction_preserves_idempotency_key_and_rerun_dedup() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let body = format!(
        r#"{{"transactions":[{{"kind":"income","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","idempotency_key":"file:1:1"}}]}}"#
    );
    let created = post_batch(&app, body).await;
    let id = created[0]["id"].as_str().unwrap();

    // 编辑内容但请求体不含 idempotency_key（幂等键不可编辑）。
    let edit_body = format!(
        r#"{{"kind":"income","amount_cents":2000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}}"#
    );
    let (status, _) = put_transaction_via_api(&app, id, &edit_body).await;
    assert_eq!(status, StatusCode::OK);

    // 编辑后重跑同批导入（同键）仍去重且返回已有 id → 不产生重复。
    let rerun_body = format!(
        r#"{{"transactions":[{{"kind":"income","amount_cents":3000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-05","idempotency_key":"file:1:1"}}]}}"#
    );
    let second = post_batch(&app, rerun_body).await;
    assert!(second[0]["success"].as_bool().unwrap());
    assert_eq!(second[0]["duplicate"], true, "编辑后同键重跑应去重");
    assert_eq!(
        second[0]["id"].as_str(),
        Some(id),
        "同键重跑应返回该笔已有 id"
    );

    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(readback["total"], 1, "编辑后重跑不应新增交易");
}

#[tokio::test]
async fn test_delete_buy_transaction_cleans_up_security_lots() {
    use tauri_app_lib::commands::insert_transaction;
    use tauri_app_lib::models::TransactionInput;

    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('acc-inv-del','美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES ('inst-del','DEL','stock','Delete','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            [],
        )
        .unwrap();
        let buy = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: "acc-inv-del".into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: Some("inst-del".into()),
            quantity: Some(10.0),
            price_cents: Some(10000),
            fee_cents: Some(500),
            idempotency_key: None,
        };
        insert_transaction(&conn, buy).unwrap();
    }

    let buy_id: String = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT id FROM transactions WHERE kind='buy' AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let lots_before: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            rusqlite::params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots_before, 1);

    let (status, _) = delete_transaction_via_api(&app, &buy_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let lots_after: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            rusqlite::params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lots_after, 0, "删除买入应清理 security_lots");
    let stx_after: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![buy_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stx_after, 0, "删除买入应清理 security_transactions");
}

#[tokio::test]
async fn test_openapi_doc_covers_delete_transaction_endpoint() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let delete = &doc["paths"]["/api/v1/transactions/{id}"]["delete"];
    assert!(delete["summary"].is_string(), "OpenAPI 应包含 DELETE 端点");
    let params = delete["parameters"]
        .as_array()
        .expect("DELETE 应声明 path 参数");
    assert!(
        params.iter().any(|p| p["name"] == "id"),
        "DELETE 端点应声明 id 路径参数"
    );
    let responses = delete["responses"].as_object().unwrap();
    assert!(responses.contains_key("204"), "应声明 204 响应");
    assert!(responses.contains_key("404"), "应声明 404 响应");
}

#[tokio::test]
async fn test_openapi_json_endpoint_returns_doc() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(doc["openapi"].as_str(), Some("3.1.0"));
    assert!(doc["info"]["title"].is_string());
    assert_eq!(doc["info"]["version"].as_str(), Some("0.1.0"));
}

#[tokio::test]
async fn test_openapi_doc_covers_all_endpoints() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let paths = doc["paths"].as_object().expect("应包含 paths 对象");

    let expected: &[(&str, &str)] = &[
        ("/api/v1/accounts", "get"),
        ("/api/v1/accounts", "post"),
        ("/api/v1/accounts/{id}", "delete"),
        ("/api/v1/accounts/balances", "get"),
        ("/api/v1/categories", "get"),
        ("/api/v1/categories", "post"),
        ("/api/v1/categories/{id}", "delete"),
        ("/api/v1/currencies", "get"),
        ("/api/v1/transactions", "get"),
        ("/api/v1/transactions/batch", "post"),
        ("/api/v1/transactions/{id}", "delete"),
        ("/api/v1/transactions/{id}", "put"),
        ("/api/v1/import/knowledge", "get"),
    ];
    for (path, method) in expected {
        assert!(
            paths.get(*path).and_then(|p| p.get(*method)).is_some(),
            "OpenAPI 文档应包含端点 {method} {path}"
        );
    }
}

#[tokio::test]
async fn test_openapi_doc_batch_wrapper_and_duplicate_field() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let schemas = doc["components"]["schemas"].as_object().unwrap();

    // batch 请求体 wrapper：{ transactions, dedup }
    let batch = &schemas["TransactionBatchInput"];
    let props = batch["properties"].as_object().unwrap();
    assert!(props.contains_key("transactions"));
    assert!(
        props.contains_key("dedup"),
        "batch wrapper 应包含 dedup 字段"
    );
    let required = batch["required"].as_array().unwrap();
    assert!(
        required.iter().any(|r| r == "transactions"),
        "transactions 应必填"
    );
    assert!(
        !required.iter().any(|r| r == "dedup"),
        "dedup 应可缺省（默认 true）"
    );

    // CreateTransactionResult 应包含 duplicate 字段
    let result = &schemas["CreateTransactionResult"];
    assert!(
        result["properties"]["duplicate"].is_object(),
        "CreateTransactionResult 应包含 duplicate 字段"
    );

    // 账户响应应包含 is_hidden（黑洞账户契约）
    let account = &schemas["Account"];
    assert!(
        account["properties"]["is_hidden"].is_object(),
        "Account 应包含 is_hidden 字段"
    );
}

#[tokio::test]
async fn test_openapi_update_transaction_input_omits_idempotency_key() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let upd = &schemas["UpdateTransactionInput"];
    let props = upd["properties"].as_object().unwrap();
    assert!(props.contains_key("kind"));
    assert!(props.contains_key("amount_cents"));
    assert!(
        !props.contains_key("idempotency_key"),
        "修改请求体不应含 idempotency_key（幂等键不可编辑）"
    );
}

#[tokio::test]
async fn test_openapi_doc_has_currencies_endpoint() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let paths = doc["paths"].as_object().unwrap();
    let currencies = &paths["/api/v1/currencies"]["get"];
    assert!(currencies["summary"].is_string());
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    assert!(schemas.contains_key("Currency"));
    assert!(schemas.contains_key("TransactionInput"));
}

#[tokio::test]
async fn test_import_knowledge_returns_ok_as_text_plain() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/knowledge")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        content_type.starts_with("text/plain"),
        "应返回纯文本，实际 content-type: {content_type}"
    );

    let bytes = body_to_bytes(response.into_body()).await;
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.trim().is_empty(), "知识内容不应为空");
    assert!(
        text.contains("/api/v1/openapi.json"),
        "知识应内嵌 OpenAPI 文档地址"
    );
}

#[tokio::test]
async fn test_import_knowledge_covers_key_conventions() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/import/knowledge")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let text = String::from_utf8(bytes).unwrap();

    let required_keywords = [
        "流入金额",
        "流出金额",
        "income",
        "expense",
        "transfer",
        "→",
        "无",
        "人民币",
        "CNY",
        "_cents",
        "YYYY-MM-DD",
        "dedup",
        "sha256",
        "account_id",
        "to_account_id",
        "currency_code",
    ];
    for kw in required_keywords {
        assert!(text.contains(kw), "导入知识应包含关键约定关键词 {kw:?}");
    }
}

#[tokio::test]
async fn test_openapi_doc_covers_knowledge_endpoint() {
    let (app, _) = setup_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = body_to_bytes(response.into_body()).await;
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let paths = doc["paths"].as_object().unwrap();
    let knowledge = &paths["/api/v1/import/knowledge"]["get"];
    assert!(knowledge["summary"].is_string());

    let responses = knowledge["responses"].as_object().unwrap();
    let ok = responses.get("200").expect("应包含 200 响应");
    let content = ok["content"].as_object().expect("200 响应应声明 content");
    assert!(
        content.contains_key("text/plain"),
        "knowledge 端点应声明 text/plain 响应"
    );
}

fn balance_of(rows: &[serde_json::Value], name: &str) -> i64 {
    rows.iter()
        .find(|r| r["account"]["name"] == name)
        .unwrap_or_else(|| panic!("应包含账户 {name}"))["balance_cents"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn test_get_account_balances_includes_seed_black_hole_accounts() {
    let (app, _) = setup_app();

    let (status, body) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("应返回 AccountBalance 数组");
    assert_eq!(rows.len(), 2, "种子应预置两个黑洞账户");
    for row in rows {
        assert_eq!(row["account"]["is_hidden"], true);
        assert_eq!(row["balance_cents"], 0);
    }
    assert!(rows.iter().any(|r| r["account"]["name"] == "无(CNY)"));
    assert!(rows.iter().any(|r| r["account"]["name"] == "无(HKD)"));
}

#[tokio::test]
async fn test_get_account_balances_applies_five_kinds_and_splits_transfer() {
    let (app, _) = setup_app();
    let cash = create_account_via_api_with_initial(&app, "现金账户", 10000).await;
    let bank = create_account_via_api_with_initial(&app, "银行账户", 0).await;
    let (_, accounts) = get_json(&app, "/api/v1/accounts").await;
    let hole = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["name"] == "无(CNY)")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let income = format!(
        r#"{{"kind":"income","amount_cents":5000,"currency_code":"CNY","account_id":"{cash}","date":"2026-01-01"}}"#
    );
    let expense = format!(
        r#"{{"kind":"expense","amount_cents":2000,"currency_code":"CNY","account_id":"{cash}","date":"2026-01-02"}}"#
    );
    let transfer = format!(
        r#"{{"kind":"transfer","amount_cents":3000,"currency_code":"CNY","account_id":"{cash}","to_account_id":"{bank}","date":"2026-01-03"}}"#
    );
    let refundable = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{cash}","date":"2026-01-04"}}"#
    );
    let hole_income = format!(
        r#"{{"kind":"income","amount_cents":700,"currency_code":"CNY","account_id":"{hole}","date":"2026-01-05"}}"#
    );
    let created = post_batch(
        &app,
        batch_body(
            &[&income, &expense, &transfer, &refundable, &hole_income],
            None,
        ),
    )
    .await;
    let expense_id = created[3]["id"].as_str().unwrap();
    let refund = format!(
        r#"{{"kind":"refund","amount_cents":400,"currency_code":"CNY","account_id":"{cash}","refund_of_transaction_id":"{expense_id}","date":"2026-01-06"}}"#
    );
    post_batch(&app, batch_body(&[&refund], None)).await;

    let (status, body) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("应返回 AccountBalance 数组");
    assert_eq!(rows.len(), 4);
    // 10000 + 5000 - 2000 - 3000 - 1000 + 400
    assert_eq!(balance_of(rows, "现金账户"), 9400);
    assert_eq!(balance_of(rows, "银行账户"), 3000);
    assert_eq!(balance_of(rows, "无(CNY)"), 700);
    assert_eq!(balance_of(rows, "无(HKD)"), 0);
    let cash_row = rows
        .iter()
        .find(|r| r["account"]["name"] == "现金账户")
        .unwrap();
    assert_eq!(cash_row["account"]["is_hidden"], false);
    let hole_row = rows
        .iter()
        .find(|r| r["account"]["name"] == "无(CNY)")
        .unwrap();
    assert_eq!(hole_row["account"]["is_hidden"], true);
}

#[tokio::test]
async fn test_get_account_balances_excludes_soft_deleted_accounts() {
    let (app, conn) = setup_app();
    let deleted = create_account_via_api_with_initial(&app, "待删除账户", 2000).await;
    create_account_via_api_with_initial(&app, "保留账户", 1000).await;
    {
        let conn = conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET is_deleted=1 WHERE id=?1",
            rusqlite::params![deleted],
        )
        .unwrap();
    }

    let (status, body) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().expect("应返回 AccountBalance 数组");
    assert!(rows.iter().any(|r| r["account"]["name"] == "保留账户"));
    assert!(!rows.iter().any(|r| r["account"]["name"] == "待删除账户"));
    assert_eq!(balance_of(rows, "保留账户"), 1000);
}

#[tokio::test]
async fn test_openapi_doc_covers_account_balances_endpoint() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let get = &doc["paths"]["/api/v1/accounts/balances"]["get"];
    assert!(get["summary"].is_string());
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("应包含 schemas");
    let balance = schemas
        .get("AccountBalance")
        .expect("OpenAPI 应包含 AccountBalance schema");
    let props = balance["properties"].as_object().unwrap();
    assert!(props.contains_key("account"));
    assert!(props.contains_key("balance_cents"));
}

#[tokio::test]
async fn test_openapi_doc_covers_list_transactions_params_and_schema() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;
    let get = &doc["paths"]["/api/v1/transactions"]["get"];
    assert!(get["summary"].is_string());
    let params = get["parameters"]
        .as_array()
        .expect("GET /transactions 应声明查询参数");
    let names: Vec<&str> = params.iter().map(|p| p["name"].as_str().unwrap()).collect();
    for expected in [
        "from",
        "to",
        "account_id",
        "kind",
        "limit",
        "page",
        "page_size",
    ] {
        assert!(
            names.contains(&expected),
            "OpenAPI 应包含查询参数 {expected}"
        );
    }
    let response_200 = get["responses"]["200"]["content"]["application/json"]["schema"]
        .as_object()
        .unwrap();
    assert_eq!(
        response_200["$ref"], "#/components/schemas/TransactionListResult",
        "响应 schema 应为 TransactionListResult"
    );
    let schemas = doc["components"]["schemas"]
        .as_object()
        .expect("应包含 schemas");
    let list_result = schemas
        .get("TransactionListResult")
        .expect("OpenAPI 应包含 TransactionListResult schema");
    let props = list_result["properties"].as_object().unwrap();
    assert!(props.contains_key("items"));
    assert!(props.contains_key("total"));
    let tx = schemas
        .get("Transaction")
        .expect("OpenAPI 应包含 Transaction schema");
    let props = tx["properties"].as_object().unwrap();
    for field in [
        "id",
        "kind",
        "amount_cents",
        "account_id",
        "date",
        "is_deleted",
    ] {
        assert!(props.contains_key(field), "Transaction 应包含字段 {field}");
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/accounts/{id}
// ---------------------------------------------------------------------------

async fn delete_account_via_api(app: &Router, id: &str) -> (StatusCode, Vec<u8>) {
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

async fn delete_category_via_api(app: &Router, id: &str) -> (StatusCode, Vec<u8>) {
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

#[tokio::test]
async fn test_openapi_doc_covers_delete_account_and_category_endpoints() {
    let (app, _) = setup_app();
    let (_, doc) = get_json(&app, "/api/v1/openapi.json").await;

    for (path, label) in [
        ("/api/v1/accounts/{id}", "账户"),
        ("/api/v1/categories/{id}", "分类"),
    ] {
        let delete = &doc["paths"][path]["delete"];
        assert!(
            delete["summary"].is_string(),
            "OpenAPI 应包含 {label} DELETE 端点"
        );
        let params = delete["parameters"]
            .as_array()
            .unwrap_or_else(|| panic!("{label} DELETE 应声明 path 参数"));
        assert!(
            params.iter().any(|p| p["name"] == "id"),
            "{label} DELETE 端点应声明 id 路径参数"
        );
        let responses = delete["responses"].as_object().unwrap();
        assert!(responses.contains_key("204"), "{label} 应声明 204 响应");
        assert!(responses.contains_key("404"), "{label} 应声明 404 响应");
    }
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
