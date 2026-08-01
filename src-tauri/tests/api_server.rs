use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

use tauri_app_lib::api_server::build_router;
use tauri_app_lib::db;

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
async fn test_openapi_doc_covers_all_six_endpoints() {
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
        ("/api/v1/categories", "get"),
        ("/api/v1/categories", "post"),
        ("/api/v1/currencies", "get"),
        ("/api/v1/transactions/batch", "post"),
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
