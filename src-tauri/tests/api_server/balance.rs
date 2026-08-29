use axum::http::StatusCode;

use crate::common::{
    balance_of, batch_body, create_account_via_api_with_initial, get_json, post_batch, setup_app,
};

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
