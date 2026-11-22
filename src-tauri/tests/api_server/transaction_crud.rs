use axum::http::StatusCode;

use tauri_app_lib::test_support;

use crate::common::{
    balance_of, batch_body, count_active_transactions, create_account_via_api,
    create_account_via_api_with_currency, delete_account_via_api, delete_transaction_via_api,
    get_json, items_of, post_batch, put_transaction_via_api, setup_app,
};

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

    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&readback);
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0]["amount_cents"], 900);
    assert_eq!(txs[0]["note"], "改后");
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
async fn test_update_buy_to_missing_instrument_returns_400_with_readable_error() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-295", "inst-295");

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-295","date":"2026-01-10","instrument_id":"inst-295","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    assert_eq!(created[0]["success"], true, "铺垫买入应成功");
    let id = created[0]["id"].as_str().unwrap().to_string();

    let body = r#"{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-295","date":"2026-01-10","instrument_id":"inst-not-exist","quantity":5.0,"price_cents":1200000,"fee_cents":0}"#;
    let (status, bytes) = put_transaction_via_api(&app, &id, body).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "引用不存在标的应返回 400 而非 500"
    );
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "Invalid");
    assert!(
        err["message"].as_str().unwrap().contains("买入标的不存在"),
        "应报买入标的不存在供回自纠: {err}"
    );

    let conn = conn.lock().unwrap();
    let (amount_cents, quantity): (i64, f64) = conn
        .query_row(
            "SELECT t.amount_cents, l.remaining_quantity FROM transactions t \
             JOIN security_lots l ON l.buy_transaction_id = t.id WHERE t.id=?1",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(amount_cents, 100000, "原交易金额不应被修改");
    assert!((quantity - 10.0).abs() < 1e-9, "原持仓批次不应被清理");
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

    let edit_body = format!(
        r#"{{"kind":"income","amount_cents":2000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-03"}}"#
    );
    let (status, _) = put_transaction_via_api(&app, id, &edit_body).await;
    assert_eq!(status, StatusCode::OK);

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
async fn test_create_sell_via_batch_succeeds_and_readable() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-sell", "inst-sell");

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-sell","date":"2026-01-10","instrument_id":"inst-sell","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let bought = post_batch(&app, buy.to_string()).await;
    assert_eq!(
        bought[0]["success"], true,
        "前置买入应成功: {:?}",
        bought[0]
    );

    let sell = r#"{"transactions":[{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-sell","date":"2026-01-20","instrument_id":"inst-sell","quantity":4.0,"price_cents":1100000,"fee_cents":0}]}"#;
    let results = post_batch(&app, sell.to_string()).await;
    assert_eq!(results[0]["success"], true, "sell 应成功: {:?}", results[0]);

    let sell_id = results[0]["id"].as_str().unwrap();
    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&readback);
    let row = txs
        .iter()
        .find(|t| t["id"].as_str() == Some(sell_id))
        .expect("创建的卖出应可读回");
    assert_eq!(row["kind"], "sell");
}

#[tokio::test]
async fn test_create_buy_with_funding_account_roundtrips_readback() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-fund", "inst-fund");
    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-fund-usd",
        "出资金账户",
        "cash",
        "USD",
        0,
    );

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-fund","date":"2026-01-10","instrument_id":"inst-fund","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"acc-fund-usd"}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    assert_eq!(
        created[0]["success"], true,
        "携带出资账户的买入应成功: {:?}",
        created[0]
    );

    let buy_id = created[0]["id"].as_str().unwrap();
    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let row = items_of(&readback)
        .iter()
        .find(|t| t["id"].as_str() == Some(buy_id))
        .expect("创建的买入应可读回");
    assert_eq!(
        row["funding_account_id"], "acc-fund-usd",
        "读回应携带出资账户 id: {row}"
    );
}

#[tokio::test]
async fn test_create_expense_with_funding_account_rejected_in_batch() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","funding_account_id":"{account_id}"}}"#
    );
    let results = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(results[0]["success"], false, "expense 不得携带出资账户");
    assert!(
        results[0]["error"]
            .as_str()
            .unwrap()
            .contains("不能携带出资账户"),
        "行级错误应可读: {results:?}"
    );
}

#[tokio::test]
async fn test_update_buy_investment_funding_returns_coded_400() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-u", "inst-u");
    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-inv-other",
        "美股二号",
        "investment",
        "USD",
        0,
    );

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-u","date":"2026-01-10","instrument_id":"inst-u","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    let id = created[0]["id"].as_str().unwrap();

    let body = r#"{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-u","date":"2026-01-10","instrument_id":"inst-u","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"acc-inv-other"}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "投资账户出资应返回 400");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "funding.account-type-unsupported");
    assert_eq!(
        err["params"],
        serde_json::json!(["investment"]),
        "params 携带被拒账户类型"
    );
}

#[tokio::test]
async fn test_update_buy_funding_currency_mismatch_returns_coded_400() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-c", "inst-c");
    let cny_cash = create_account_via_api(&app, "人民币现金").await;

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-c","date":"2026-01-10","instrument_id":"inst-c","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    let id = created[0]["id"].as_str().unwrap();

    let body = format!(
        r#"{{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-c","date":"2026-01-10","instrument_id":"inst-c","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"{cny_cash}"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, id, &body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "跨币种出资应返回 400");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "funding.currency-mismatch");
    assert_eq!(
        err["params"],
        serde_json::json!(["CNY", "USD"]),
        "params 顺序 = 消息动态值顺序（出资币种 → 交易币种）"
    );
}

#[tokio::test]
async fn test_update_buy_deleted_funding_account_returns_coded_404() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-d", "inst-d");
    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-fund-del",
        "待删出资金",
        "cash",
        "USD",
        0,
    );
    let (status, _) = delete_account_via_api(&app, "acc-fund-del").await;
    assert_eq!(status, StatusCode::NO_CONTENT, "铺垫账户删除应成功");

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-d","date":"2026-01-10","instrument_id":"inst-d","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    let id = created[0]["id"].as_str().unwrap();

    let body = r#"{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-d","date":"2026-01-10","instrument_id":"inst-d","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"acc-fund-del"}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, body).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "软删出资账户应返回 404");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["kind"], "NotFound");
    assert_eq!(err["code"], "funding.account-not-found");
}

#[tokio::test]
async fn test_create_buy_with_to_account_rejected_with_readable_error() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-ta", "inst-ta");

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-ta","date":"2026-01-10","instrument_id":"inst-ta","quantity":10.0,"price_cents":1000000,"fee_cents":0,"to_account_id":"acc-inv-ta"}]}"#;
    let results = post_batch(&app, buy.to_string()).await;
    assert_eq!(results[0]["success"], false, "携带转入账户的买入应被拒绝");
    let error = results[0]["error"].as_str().unwrap();
    assert!(
        error.contains("买入不能携带转入账户"),
        "行级错误应对准守卫文案: {error}"
    );
    assert_eq!(
        count_active_transactions(&conn.lock().unwrap()),
        0,
        "被拒的买入不应落交易行"
    );
}

#[tokio::test]
async fn test_create_sell_with_to_account_rejected_with_readable_error() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-ta-s", "inst-ta-s");

    let sell = r#"{"transactions":[{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-ta-s","date":"2026-01-10","instrument_id":"inst-ta-s","quantity":5.0,"price_cents":1000000,"fee_cents":0,"to_account_id":"acc-inv-ta-s"}]}"#;
    let results = post_batch(&app, sell.to_string()).await;
    assert_eq!(results[0]["success"], false, "携带转入账户的卖出应被拒绝");
    let error = results[0]["error"].as_str().unwrap();
    assert!(
        error.contains("卖出不能携带转入账户"),
        "行级错误应对准守卫文案: {error}"
    );
    assert_eq!(
        count_active_transactions(&conn.lock().unwrap()),
        0,
        "被拒的卖出不应落交易行"
    );
}

#[tokio::test]
async fn test_update_buy_with_to_account_returns_coded_400() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-ta-u", "inst-ta-u");

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-ta-u","date":"2026-01-10","instrument_id":"inst-ta-u","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    let id = created[0]["id"].as_str().unwrap();

    let body = r#"{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-ta-u","date":"2026-01-10","instrument_id":"inst-ta-u","quantity":10.0,"price_cents":1000000,"fee_cents":0,"to_account_id":"acc-inv-ta-u"}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "携带转入账户应返回 400");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.buy-to-account-forbidden");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("买入不能携带转入账户"),
        "拒绝文案应对准买入，实际: {err}"
    );
}

#[tokio::test]
async fn test_update_sell_with_to_account_returns_coded_400() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-ta-us", "inst-ta-us");

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-ta-us","date":"2026-01-10","instrument_id":"inst-ta-us","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    assert_eq!(created[0]["success"], true, "建仓买入应成功");
    let sell = r#"{"transactions":[{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-ta-us","date":"2026-01-20","instrument_id":"inst-ta-us","quantity":4.0,"price_cents":1100000,"fee_cents":0}]}"#;
    let created = post_batch(&app, sell.to_string()).await;
    let id = created[0]["id"].as_str().unwrap();

    let body = r#"{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-ta-us","date":"2026-01-20","instrument_id":"inst-ta-us","quantity":4.0,"price_cents":1100000,"fee_cents":0,"to_account_id":"acc-inv-ta-us"}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "携带转入账户应返回 400");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.sell-to-account-forbidden");
    assert!(
        err["message"]
            .as_str()
            .unwrap()
            .contains("卖出不能携带转入账户"),
        "拒绝文案应对准卖出，实际: {err}"
    );
}

#[tokio::test]
async fn test_create_sell_with_funding_account_roundtrips_readback() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-sell", "inst-sell");
    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-sell","date":"2026-01-10","instrument_id":"inst-sell","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let buy_results = post_batch(&app, buy.to_string()).await;
    assert_eq!(
        buy_results[0]["success"], true,
        "建仓买入应成功: {:?}",
        buy_results[0]
    );

    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-fund-sell",
        "回卡出资金",
        "bank",
        "USD",
        0,
    );
    let sell = r#"{"transactions":[{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-sell","date":"2026-01-11","instrument_id":"inst-sell","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"acc-fund-sell"}]}"#;
    let created = post_batch(&app, sell.to_string()).await;
    assert_eq!(
        created[0]["success"], true,
        "携带出资账户的卖出应成功: {:?}",
        created[0]
    );

    let sell_id = created[0]["id"].as_str().unwrap();
    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let row = items_of(&readback)
        .iter()
        .find(|t| t["id"].as_str() == Some(sell_id))
        .expect("创建的卖出应可读回");
    assert_eq!(
        row["funding_account_id"], "acc-fund-sell",
        "读回应携带出资账户 id: {row}"
    );
}

#[tokio::test]
async fn test_update_sell_funding_swap_and_clear_roundtrip() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-swap", "inst-swap");
    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-fund-a",
        "出资卡A",
        "bank",
        "USD",
        0,
    );
    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-fund-b",
        "出资卡B",
        "bank",
        "USD",
        0,
    );
    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-swap","date":"2026-01-10","instrument_id":"inst-swap","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let buy_results = post_batch(&app, buy.to_string()).await;
    assert_eq!(
        buy_results[0]["success"], true,
        "建仓买入应成功: {:?}",
        buy_results[0]
    );

    let sell = r#"{"transactions":[{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-swap","date":"2026-01-11","instrument_id":"inst-swap","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"acc-fund-a"}]}"#;
    let created = post_batch(&app, sell.to_string()).await;
    let id = created[0]["id"].as_str().unwrap();

    let swap = r#"{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-swap","date":"2026-01-11","instrument_id":"inst-swap","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"acc-fund-b"}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, swap).await;
    assert_eq!(status, StatusCode::OK, "换出资账户应成功: {:?}", bytes);
    let (_, after_swap) = get_json(&app, "/api/v1/transactions").await;
    let row = items_of(&after_swap)
        .iter()
        .find(|t| t["id"].as_str() == Some(id))
        .expect("修改后的卖出应可读回");
    assert_eq!(
        row["funding_account_id"], "acc-fund-b",
        "换端后读回新值: {row}"
    );

    let clear = r#"{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-swap","date":"2026-01-11","instrument_id":"inst-swap","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":null}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, clear).await;
    assert_eq!(status, StatusCode::OK, "清空出资账户应成功: {:?}", bytes);
    let (_, after_clear) = get_json(&app, "/api/v1/transactions").await;
    let row = items_of(&after_clear)
        .iter()
        .find(|t| t["id"].as_str() == Some(id))
        .expect("清空后的卖出应可读回");
    assert!(
        row["funding_account_id"].is_null(),
        "清空后读回 null: {row}"
    );
}

#[tokio::test]
async fn test_update_sell_investment_funding_returns_coded_400() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-se", "inst-se");
    test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-inv-other",
        "美股二号",
        "investment",
        "USD",
        0,
    );
    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-se","date":"2026-01-10","instrument_id":"inst-se","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let buy_results = post_batch(&app, buy.to_string()).await;
    assert_eq!(
        buy_results[0]["success"], true,
        "建仓买入应成功: {:?}",
        buy_results[0]
    );

    let sell = r#"{"transactions":[{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-se","date":"2026-01-11","instrument_id":"inst-se","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, sell.to_string()).await;
    let id = created[0]["id"].as_str().unwrap();

    let body = r#"{"kind":"sell","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-se","date":"2026-01-11","instrument_id":"inst-se","quantity":10.0,"price_cents":1000000,"fee_cents":0,"funding_account_id":"acc-inv-other"}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "投资账户出资应返回 400");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "funding.account-type-unsupported");
    assert_eq!(
        err["params"],
        serde_json::json!(["investment"]),
        "params 携带被拒账户类型"
    );
}

#[tokio::test]
async fn test_update_convert_in_place_rebuilds_carry() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-cv-put", "基金户", "investment", "CNY", 0);
        test_support::seed_instrument(
            &conn,
            "inst-cv-p-out",
            "006793",
            "转出基金",
            "CNY",
            "unknown",
        );
        test_support::seed_instrument(
            &conn,
            "inst-cv-p-in",
            "519700",
            "转入基金",
            "CNY",
            "unknown",
        );
    }
    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-put","date":"2026-01-10","instrument_id":"inst-cv-p-out","quantity":10.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");
    let convert = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-put","date":"2026-02-01","instrument_id":"inst-cv-p-out","quantity":10.0,"to_instrument_id":"inst-cv-p-in","to_quantity":10.0,"out_amount_cents":1100,"in_amount_cents":1100,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[convert], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");
    let id = created[0]["id"].as_str().unwrap();

    let body = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-put","date":"2026-02-05","instrument_id":"inst-cv-p-out","quantity":2.0,"to_instrument_id":"inst-cv-p-in","to_quantity":2.0,"out_amount_cents":240,"in_amount_cents":240,"fee_cents":0}"#;
    let (status, bytes) = put_transaction_via_api(&app, id, body).await;
    assert_eq!(status, StatusCode::OK, "就地修改转换应成功: {bytes:?}");
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["kind"], "convert");
    assert_eq!(updated["date"], "2026-02-05");

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let row = items_of(&list)
        .iter()
        .find(|t| t["id"].as_str() == Some(id))
        .expect("修改后的转换应可读回");
    assert_eq!(row["convert"]["out_amount_cents"], 240);
    assert_eq!(row["convert"]["to_instrument_id"], "inst-cv-p-in");
}

#[tokio::test]
async fn test_update_kind_to_or_from_convert_returns_coded_400() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-cv-kind", "基金户", "investment", "CNY", 0);
        test_support::seed_instrument(
            &conn,
            "inst-cv-k-out",
            "006793",
            "转出基金",
            "CNY",
            "unknown",
        );
        test_support::seed_instrument(
            &conn,
            "inst-cv-k-in",
            "519700",
            "转入基金",
            "CNY",
            "unknown",
        );
    }
    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-kind","date":"2026-01-10","instrument_id":"inst-cv-k-out","quantity":10.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    let buy_id = created[0]["id"].as_str().unwrap().to_string();

    let as_convert = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-kind","date":"2026-02-01","instrument_id":"inst-cv-k-out","quantity":1.0,"to_instrument_id":"inst-cv-k-in","to_quantity":1.0,"out_amount_cents":100,"in_amount_cents":100,"fee_cents":0}"#;
    let (status, bytes) = put_transaction_via_api(&app, &buy_id, as_convert).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.convert-kind-change-forbidden");

    let convert = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-kind","date":"2026-02-01","instrument_id":"inst-cv-k-out","quantity":10.0,"to_instrument_id":"inst-cv-k-in","to_quantity":10.0,"out_amount_cents":1100,"in_amount_cents":1100,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[convert], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");
    let convert_id = created[0]["id"].as_str().unwrap().to_string();
    let as_buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-kind","date":"2026-01-10","instrument_id":"inst-cv-k-out","quantity":1.0,"price_cents":10000,"fee_cents":0}"#;
    let (status, bytes) = put_transaction_via_api(&app, &convert_id, as_buy).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.convert-kind-change-forbidden");

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let ids: Vec<&str> = items_of(&list)
        .iter()
        .filter_map(|t| t["id"].as_str())
        .collect();
    assert!(ids.contains(&buy_id.as_str()), "被拒的买入仍在");
    assert!(ids.contains(&convert_id.as_str()), "被拒的转换仍在");
}

#[tokio::test]
async fn test_delete_convert_restores_positions_and_cascades_sell() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-cv-del", "基金户", "investment", "CNY", 0);
        test_support::seed_instrument(
            &conn,
            "inst-cv-d-out",
            "006793",
            "转出基金",
            "CNY",
            "unknown",
        );
        test_support::seed_instrument(
            &conn,
            "inst-cv-d-in",
            "519700",
            "转入基金",
            "CNY",
            "unknown",
        );
    }
    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-del","date":"2026-01-10","instrument_id":"inst-cv-d-out","quantity":10.0,"price_cents":10000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[buy], None)).await;
    let buy_id = created[0]["id"].as_str().unwrap().to_string();
    let convert = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-del","date":"2026-02-01","instrument_id":"inst-cv-d-out","quantity":10.0,"to_instrument_id":"inst-cv-d-in","to_quantity":10.0,"out_amount_cents":1100,"in_amount_cents":1100,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[convert], None)).await;
    let convert_id = created[0]["id"].as_str().unwrap().to_string();
    let sell = r#"{"kind":"sell","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-del","date":"2026-02-10","instrument_id":"inst-cv-d-in","quantity":4.0,"price_cents":15000,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[sell], None)).await;
    let sell_id = created[0]["id"].as_str().unwrap().to_string();

    let (status, bytes) = delete_transaction_via_api(&app, &convert_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "删除转换应成功: {bytes:?}");

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let ids: Vec<&str> = items_of(&list)
        .iter()
        .filter_map(|t| t["id"].as_str())
        .collect();
    assert!(!ids.contains(&convert_id.as_str()), "删除的转换不再可读回");
    assert!(
        !ids.contains(&sell_id.as_str()),
        "级联软删的 sell 不再可读回"
    );

    let (status, _) = delete_transaction_via_api(&app, &buy_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_delete_convert_guarded_by_later_convert_returns_coded_400() {
    let (app, conn) = setup_app();
    {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-cv-chain", "基金户", "investment", "CNY", 0);
        test_support::seed_instrument(
            &conn,
            "inst-cv-c-out",
            "006793",
            "转出基金",
            "CNY",
            "unknown",
        );
        test_support::seed_instrument(
            &conn,
            "inst-cv-c-in",
            "519700",
            "转入基金",
            "CNY",
            "unknown",
        );
    }
    let buy = r#"{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-chain","date":"2026-01-10","instrument_id":"inst-cv-c-out","quantity":10.0,"price_cents":10000,"fee_cents":0}"#;
    post_batch(&app, batch_body(&[buy], None)).await;
    let first = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-chain","date":"2026-02-01","instrument_id":"inst-cv-c-out","quantity":10.0,"to_instrument_id":"inst-cv-c-in","to_quantity":10.0,"out_amount_cents":1100,"in_amount_cents":1100,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[first], None)).await;
    let first_id = created[0]["id"].as_str().unwrap().to_string();
    let second = r#"{"kind":"convert","amount_cents":0,"currency_code":"CNY","account_id":"acc-cv-chain","date":"2026-03-01","instrument_id":"inst-cv-c-in","quantity":10.0,"to_instrument_id":"inst-cv-c-out","to_quantity":10.0,"out_amount_cents":1100,"in_amount_cents":1100,"fee_cents":0}"#;
    let created = post_batch(&app, batch_body(&[second], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");
    let second_id = created[0]["id"].as_str().unwrap().to_string();

    let (status, bytes) = delete_transaction_via_api(&app, &first_id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "前腿删除应被拒绝");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "trade.consumed-by-convert-delete");

    let (status, _) = delete_transaction_via_api(&app, &second_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = delete_transaction_via_api(&app, &first_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    assert_eq!(items_of(&list).len(), 1, "只剩建仓买入");
}

#[tokio::test]
async fn test_update_transaction_currency_mismatch_returns_coded_400() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api_with_currency(&app, "港币户", "HKD").await;
    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        test_support::seed_fx_rate_history(&guard, "fxh-mism", "HKD", "CNY", "2026-06-29", 0.9);
    }
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"HKD","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");
    let id = created[0]["id"].as_str().unwrap().to_string();

    let mismatch = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, &id, &mismatch).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bytes:?}");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "transaction.currency-mismatch");
    assert_eq!(err["params"], serde_json::json!(["HKD", "CNY"]));

    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let items = items_of(&body);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["currency_code"], "HKD");
}

#[tokio::test]
async fn test_batch_import_foreign_currency_row_readback_exposes_fx_trace() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api_with_currency(&app, "港币账户", "HKD").await;
    let cny_account_id = create_account_via_api(&app, "现金账户").await;
    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        test_support::seed_fx_rate_history(&guard, "fxh-api", "HKD", "CNY", "2026-06-29", 0.9);
    }

    let fx_tx = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"HKD","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let cny_tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{cny_account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&fx_tx, &cny_tx], None)).await;
    assert_eq!(created[0]["success"], true, "外币行应有汇率可折算");
    assert_eq!(created[1]["success"], true);

    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&body);
    assert_eq!(txs.len(), 2);
    for tx in txs {
        if tx["currency_code"] == "HKD" {
            assert_eq!(tx["fx_rate_used"], 0.9, "留痕 = 本笔使用的序列汇率值");
            assert_eq!(tx["fx_rate_source"], "series", "来源 = 序列命中");
        } else {
            assert_eq!(
                tx["fx_rate_used"],
                serde_json::Value::Null,
                "同币种不折算：无汇率留痕"
            );
            assert_eq!(
                tx["fx_rate_source"],
                serde_json::Value::Null,
                "同币种不折算：无来源留痕"
            );
        }
    }
}

#[tokio::test]
async fn test_update_foreign_currency_row_reuses_fx_trace_when_series_missing() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api_with_currency(&app, "港币编辑户", "HKD").await;
    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        test_support::seed_fx_rate_history(&guard, "fxh-upd", "HKD", "CNY", "2026-06-29", 0.9);
    }
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"HKD","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(created[0]["success"], true, "{created:?}");
    let id = created[0]["id"].as_str().unwrap().to_string();

    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        guard.execute("DELETE FROM fx_rate_history", []).unwrap();
    }

    let note_only = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"HKD","account_id":"{account_id}","date":"2026-07-01","note":"只改备注"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, &id, &note_only).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "note-only 编辑不应重查序列: {bytes:?}"
    );
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["fx_rate_used"], 0.9);
    assert_eq!(updated["fx_rate_source"], "series");
    assert_eq!(updated["note"], "只改备注");

    let amount_changed = format!(
        r#"{{"kind":"expense","amount_cents":2000,"currency_code":"HKD","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, &id, &amount_changed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bytes:?}");
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(err["code"], "fx.rate-missing");
}

#[tokio::test]
async fn test_batch_import_row_with_explicit_fx_rate_readback_marks_explicit() {
    let (app, _conn) = setup_app();
    let account_id = create_account_via_api_with_currency(&app, "港币账户", "HKD").await;
    let cny_account_id = create_account_via_api(&app, "现金账户").await;

    let hkd_tx = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"HKD","account_id":"{account_id}","date":"2022-01-21","fx_rate":0.88}}"#
    );
    let cny_tx = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{cny_account_id}","date":"2022-01-21"}}"#
    );
    let created = post_batch(&app, batch_body(&[&hkd_tx, &cny_tx], None)).await;
    assert_eq!(created[0]["success"], true, "带显式汇率的行应照常落库");
    assert_eq!(created[1]["success"], true, "不带 fxRate 的行行为不变");

    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&body);
    for tx in txs {
        if tx["currency_code"] == "HKD" {
            assert_eq!(tx["fx_rate_used"], 0.88, "留痕 = 显式给定值本身");
            assert_eq!(tx["fx_rate_source"], "explicit", "来源标为显式");
        } else {
            assert_eq!(tx["fx_rate_used"], serde_json::Value::Null);
            assert_eq!(tx["fx_rate_source"], serde_json::Value::Null);
        }
    }
}

#[tokio::test]
async fn test_batch_import_illegal_explicit_fx_rate_rejected_not_persisted() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api_with_currency(&app, "港币账户", "HKD").await;
    let cny_account_id = create_account_via_api(&app, "现金账户").await;

    let zero = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"HKD","account_id":"{account_id}","date":"2022-01-21","fx_rate":0}}"#
    );
    let same_currency = format!(
        r#"{{"kind":"expense","amount_cents":500,"currency_code":"CNY","account_id":"{cny_account_id}","date":"2022-01-21","fx_rate":1.0}}"#
    );
    let created = post_batch(&app, batch_body(&[&zero, &same_currency], None)).await;
    assert_eq!(created[0]["success"], false, "非正显式汇率应失败");
    assert!(created[0]["error"].as_str().unwrap().contains("大于 0"));
    assert_eq!(created[1]["success"], false, "同币种携带显式汇率应失败");
    assert!(
        created[1]["error"].as_str().unwrap().contains("方向不符"),
        "实际: {:?}",
        created[1]["error"]
    );

    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(
            count_active_transactions(&guard),
            0,
            "非法行不落库：库内无交易行"
        );
    }
}

#[tokio::test]
async fn test_batch_import_explicit_fx_rate_row_idempotent_rerun() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api_with_currency(&app, "港币账户", "HKD").await;

    let row = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"HKD","account_id":"{account_id}","date":"2022-01-21","fx_rate":0.88,"idempotency_key":"moomoo-2022-01-21-1"}}"#
    );
    let first = post_batch(&app, batch_body(&[&row], None)).await;
    assert_eq!(first[0]["success"], true);
    assert_eq!(first[0]["duplicate"], false);

    let rerun = post_batch(&app, batch_body(&[&row], None)).await;
    assert_eq!(rerun[0]["success"], true, "同键重跑仍成功");
    assert_eq!(rerun[0]["duplicate"], true, "同键重跑判为重复跳过");

    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(count_active_transactions(&guard), 1, "同批重跑只落一份");
    }
}

#[tokio::test]
async fn test_combined_payment_roundtrip_readback_and_balances() {
    let (app, conn) = setup_app();
    let wallet = create_account_via_api(&app, "小金库").await;
    let balance = create_account_via_api(&app, "余额").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":28160,"currency_code":"CNY","date":"2026-07-01","funding":[{{"account_id":"{wallet}","amount_cents":2853,"label":"定金"}},{{"account_id":"{balance}","amount_cents":25307}}]}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(
        created[0]["success"], true,
        "组合支付应成功: {:?}",
        created[0]
    );
    let id = created[0]["id"].as_str().unwrap();

    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let rows = items_of(&readback);
    assert_eq!(rows.len(), 1, "列表按交易计数（组合支付计 1 笔）");
    let row = &rows[0];
    assert_eq!(row["id"], id);
    assert_eq!(
        row["amount_cents"], 28160,
        "组合支付行按交易计 1 笔、金额不拆分"
    );
    assert_eq!(
        row["account_id"],
        serde_json::Value::Null,
        "分解行 account_id 读回 null"
    );
    let fundings = row["fundings"].as_array().expect("读回携带出资项数组");
    assert_eq!(fundings.len(), 2);
    assert_eq!(fundings[0]["account_id"], wallet);
    assert_eq!(fundings[0]["amount_cents"], 2853);
    assert_eq!(fundings[0]["label"], "定金");
    assert_eq!(fundings[1]["account_id"], balance);
    assert_eq!(fundings[1]["amount_cents"], 25307);

    let (_, b) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(balance_of(b.as_array().unwrap(), "小金库"), -2853);
    assert_eq!(balance_of(b.as_array().unwrap(), "余额"), -25307);

    {
        let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
        let sub: i64 = guard
            .query_row(
                "SELECT COUNT(*) FROM transaction_fundings WHERE transaction_id=?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sub, 2, "子行按顺序位落库");
    }
}

#[tokio::test]
async fn test_combined_payment_with_account_id_returns_coded_400() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","funding":[{{"account_id":"{account_id}","amount_cents":1000}}]}}"#
    );
    let results = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(results[0]["success"], false);
    assert!(
        results[0]["error"]
            .as_str()
            .unwrap()
            .contains("不可指定 account_id"),
        "互斥错误应可读: {results:?}"
    );
}

#[tokio::test]
async fn test_combined_payment_sum_mismatch_returns_coded_error() {
    let (app, _) = setup_app();
    let wallet = create_account_via_api(&app, "小金库").await;
    let balance = create_account_via_api(&app, "余额").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","date":"2026-07-01","funding":[{{"account_id":"{wallet}","amount_cents":600}},{{"account_id":"{balance}","amount_cents":600}}]}}"#
    );
    let results = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(results[0]["success"], false);
    let err = results[0]["error"].as_str().unwrap();
    assert!(
        err.contains("出资分解合计（1200）") && err.contains("交易金额（1000）"),
        "{err}"
    );
}

#[tokio::test]
async fn test_combined_payment_label_too_long_returns_coded_error() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "小金库").await;
    let long_label = "标".repeat(51);
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","date":"2026-07-01","funding":[{{"account_id":"{account_id}","amount_cents":1000,"label":"{long_label}"}}]}}"#
    );
    let results = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(results[0]["success"], false);
    assert!(results[0]["error"].as_str().unwrap().contains("50 字"));
}

#[tokio::test]
async fn test_update_single_to_multi_conversion_roundtrips() {
    let (app, _) = setup_app();
    let wallet = create_account_via_api(&app, "小金库").await;
    let balance = create_account_via_api(&app, "余额").await;

    let tx = format!(
        r#"{{"kind":"expense","amount_cents":28160,"currency_code":"CNY","account_id":"{wallet}","date":"2026-07-01"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap().to_string();

    let body = format!(
        r#"{{"kind":"expense","amount_cents":28160,"currency_code":"CNY","date":"2026-07-01","funding":[{{"account_id":"{wallet}","amount_cents":2853}},{{"account_id":"{balance}","amount_cents":25307}}]}}"#
    );
    let (status, bytes) = put_transaction_via_api(&app, &id, &body).await;
    assert_eq!(status, StatusCode::OK);
    let updated: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(updated["account_id"], serde_json::Value::Null);
    assert_eq!(updated["fundings"].as_array().unwrap().len(), 2);

    let (_, balances) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(balance_of(balances.as_array().unwrap(), "小金库"), -2853);
    assert_eq!(balance_of(balances.as_array().unwrap(), "余额"), -25307);
}

#[tokio::test]
async fn test_refund_of_combined_payment_derives_breakdown_by_ratio() {
    let (app, _) = setup_app();
    let wallet = create_account_via_api(&app, "小金库").await;
    let balance = create_account_via_api(&app, "余额").await;

    let tx = format!(
        r#"{{"kind":"expense","amount_cents":28160,"currency_code":"CNY","date":"2026-07-01","funding":[{{"account_id":"{wallet}","amount_cents":2853}},{{"account_id":"{balance}","amount_cents":25307}}]}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let expense_id = created[0]["id"].as_str().unwrap();

    let refund = format!(
        r#"{{"kind":"refund","amount_cents":7040,"currency_code":"CNY","date":"2026-07-02","refund_of_transaction_id":"{expense_id}"}}"#
    );
    let results = post_batch(&app, batch_body(&[&refund], None)).await;
    assert_eq!(results[0]["success"], true, "退款应成功: {:?}", results[0]);
    let refund_id = results[0]["id"].as_str().unwrap();

    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let row = items_of(&readback)
        .iter()
        .find(|t| t["id"].as_str() == Some(refund_id))
        .expect("退款行应可读回");
    assert_eq!(
        row["account_id"],
        serde_json::Value::Null,
        "原支出为分解行：退款无单一账户可继承"
    );
    let fundings = row["fundings"].as_array().expect("缺省按比例派生分解");
    assert_eq!(fundings.len(), 2);
    assert_eq!(fundings[0]["account_id"], wallet);
    assert_eq!(fundings[0]["amount_cents"], 714, "floor 713 + 尾差 1 分");
    assert_eq!(fundings[0]["derived"], true, "派生条目带标注");
    assert_eq!(fundings[1]["account_id"], balance);
    assert_eq!(fundings[1]["amount_cents"], 6326);
}

#[tokio::test]
async fn test_delete_combined_payment_restores_all_funded_balances() {
    let (app, _) = setup_app();
    let wallet = create_account_via_api(&app, "小金库").await;
    let balance = create_account_via_api(&app, "余额").await;

    let tx = format!(
        r#"{{"kind":"expense","amount_cents":28160,"currency_code":"CNY","date":"2026-07-01","funding":[{{"account_id":"{wallet}","amount_cents":2853}},{{"account_id":"{balance}","amount_cents":25307}}]}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap();

    let (status, _) = delete_transaction_via_api(&app, id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, balances) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(balance_of(balances.as_array().unwrap(), "小金库"), 0);
    assert_eq!(balance_of(balances.as_array().unwrap(), "余额"), 0);
}

#[tokio::test]
async fn test_batch_import_source_order_no_roundtrips_and_put_clears() {
    let (app, _conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金账户").await;
    let tx1 = format!(
        r#"{{"kind":"expense","amount_cents":28160,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","source_order_no":"JD-9001","idempotency_key":"src:1"}}"#
    );
    let tx2 = format!(
        r#"{{"kind":"expense","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","source_order_no":"JD-9001","idempotency_key":"src:2"}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx1, &tx2], None)).await;
    assert_eq!(created[0]["duplicate"], false);
    assert_eq!(created[1]["duplicate"], false);

    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let rows = items_of(&body);
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|t| t["source_order_no"] == "JD-9001"));

    let id = rows[0]["id"].as_str().unwrap();
    let put = format!(
        r#"{{"kind":"expense","amount_cents":28160,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let (status, _) = put_transaction_via_api(&app, id, &put).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let rows = items_of(&body);
    let cleared = rows.iter().find(|t| t["id"] == *id).unwrap();
    assert!(
        cleared["source_order_no"].is_null(),
        "PUT 缺省应清除来源订单号"
    );
    let untouched = rows.iter().find(|t| t["id"] != *id).unwrap();
    assert_eq!(untouched["source_order_no"], "JD-9001", "未修改行保持原值");
}

#[tokio::test]
async fn test_multi_item_order_purchases_roundtrip_readback() {
    let (app, conn) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":10000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","note":"京东自营","purchases":[{{"name":"猫粮","quantity":1,"unit_price_cents":5990}},{{"name":"洗衣液","quantity":2}},{{"name":"纸巾","quantity":3}}]}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(
        created[0]["success"], true,
        "多商品订单应成功: {:?}",
        created[0]
    );
    let id = created[0]["id"].as_str().unwrap();

    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let rows = items_of(&readback);
    assert_eq!(rows.len(), 1, "列表按交易计数（3 件商品仍计 1 笔）");
    assert_eq!(rows[0]["id"], id);
    let purchases = rows[0]["purchases"].as_array().expect("读回携带购买项数组");
    assert_eq!(purchases.len(), 3);
    assert_eq!(purchases[0]["name"], "猫粮");
    assert_eq!(purchases[0]["quantity"], 1);
    assert_eq!(purchases[0]["unit_price_cents"], 5990);
    assert_eq!(purchases[1]["name"], "洗衣液");
    assert!(
        purchases[1]["unit_price_cents"].is_null(),
        "源单只给总额时单价留空照常落库与读回"
    );
    assert_eq!(purchases[2]["name"], "纸巾");

    let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
    let sub: i64 = guard
        .query_row(
            "SELECT COUNT(*) FROM transaction_purchases WHERE transaction_id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sub, 3);
}

#[tokio::test]
async fn test_purchases_on_transfer_rejected_coded() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;
    let to_account = create_account_via_api(&app, "银行").await;
    let tx = format!(
        r#"{{"kind":"transfer","amount_cents":1000,"currency_code":"CNY","account_id":"{account_id}","to_account_id":"{to_account}","date":"2026-07-01","purchases":[{{"name":"猫粮","quantity":1}}]}}"#
    );
    let results = post_batch(&app, batch_body(&[&tx], None)).await;
    assert_eq!(results[0]["success"], false);
    let err = results[0]["error"].as_str().unwrap();
    assert!(
        err.contains("不能携带购买项"),
        "准入错误应可读自纠: {results:?}"
    );
}

#[tokio::test]
async fn test_update_replaces_purchases_full_semantics() {
    let (app, _) = setup_app();
    let account_id = create_account_via_api(&app, "现金").await;
    let tx = format!(
        r#"{{"kind":"expense","amount_cents":10000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","purchases":[{{"name":"猫粮","quantity":1}},{{"name":"洗衣液","quantity":2}}]}}"#
    );
    let created = post_batch(&app, batch_body(&[&tx], None)).await;
    let id = created[0]["id"].as_str().unwrap().to_string();

    let put = format!(
        r#"{{"kind":"expense","amount_cents":10000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01","purchases":[{{"name":"纸巾","quantity":3}}]}}"#
    );
    let (status, _) = put_transaction_via_api(&app, &id, &put).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let rows = items_of(&body);
    let purchases = rows[0]["purchases"].as_array().unwrap();
    assert_eq!(purchases.len(), 1);
    assert_eq!(purchases[0]["name"], "纸巾");

    let clear = format!(
        r#"{{"kind":"expense","amount_cents":10000,"currency_code":"CNY","account_id":"{account_id}","date":"2026-07-01"}}"#
    );
    let (status, _) = put_transaction_via_api(&app, &id, &clear).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get_json(&app, "/api/v1/transactions").await;
    let rows = items_of(&body);
    assert!(
        rows[0]["purchases"].as_array().unwrap().is_empty(),
        "PUT 缺省应整体移除购买项"
    );
}
