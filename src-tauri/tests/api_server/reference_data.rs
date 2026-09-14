use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::common::{
    body_to_bytes, count_rows, create_account_json, create_account_via_api,
    create_category_via_api, read_account_by_id, setup_app,
};

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

// ---------------------------------------------------------------------------
// 信用卡档案字段（spec #1327 / ADR-0119）：创建侧携带与拒绝（壳三件套 + 接线证明）。
// ---------------------------------------------------------------------------

/// 创建信用卡账户携带三个档案字段 → 201，且读出口读回同值。
/// 接线证明：写入经 HTTP 进入账户写入接缝，直接可观察结果是列表读得到。
#[tokio::test]
async fn test_create_credit_account_persists_credit_terms() {
    let (app, _) = setup_app();
    let body = r#"{"name":"招行信用卡","type":"credit","currency_code":"CNY","initial_balance_cents":-12345,"credit_limit_cents":5000000,"statement_day":5,"due_day":25}"#;
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
    let id: String = serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();

    let account = read_account_by_id(&app, &id).await;
    assert_eq!(account["type"], "credit");
    assert_eq!(account["credit_limit_cents"], 5_000_000);
    assert_eq!(account["statement_day"], 5);
    assert_eq!(account["due_day"], 25);
}

/// 未携带档案字段的信用卡账户：三个字段读回 `null`（「未设置」与「额度为 0」不是
/// 同一状态），且创建不报错。
#[tokio::test]
async fn test_create_credit_account_without_credit_terms_returns_nulls() {
    let (app, _) = setup_app();
    let body = r#"{"name":"无档案信用卡","type":"credit","currency_code":"CNY"}"#;
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
    let id: String = serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();

    let account = read_account_by_id(&app, &id).await;
    assert!(account["credit_limit_cents"].is_null());
    assert!(account["statement_day"].is_null());
    assert!(account["due_day"].is_null());
}

/// 非信用卡账户携带档案字段 → 400 + 码化错误（账户类型与属性的对应关系不被破坏）。
#[tokio::test]
async fn test_create_non_credit_account_with_credit_terms_returns_400() {
    let (app, _) = setup_app();
    let body =
        r#"{"name":"现金","type":"cash","currency_code":"CNY","credit_limit_cents":5000000}"#;
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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let err: serde_json::Value =
        serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert_eq!(err["code"], "account.credit-attribute-not-applicable");
}

/// 越界档案值 → 400 + 各自错误码（维度即矩阵：额度为正、账单日与还款日各自 1–31）。
#[tokio::test]
async fn test_create_credit_account_rejects_out_of_range_terms() {
    let (app, _) = setup_app();
    for (body, code) in [
        (
            r#"{"name":"额度为零","type":"credit","currency_code":"CNY","credit_limit_cents":0}"#,
            "account.credit-limit-invalid",
        ),
        (
            r#"{"name":"账单日越界","type":"credit","currency_code":"CNY","statement_day":32}"#,
            "account.statement-day-invalid",
        ),
        (
            r#"{"name":"还款日越界","type":"credit","currency_code":"CNY","due_day":0}"#,
            "account.due-day-invalid",
        ),
    ] {
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
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{code}");
        let err: serde_json::Value =
            serde_json::from_slice(&body_to_bytes(response.into_body()).await).unwrap();
        assert_eq!(err["code"], code);
    }
}
