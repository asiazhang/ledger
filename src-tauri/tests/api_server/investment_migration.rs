use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ledger_investment::{InstrumentType, Market};
use tauri_app_lib::test_support;
use tower::ServiceExt;

use crate::common::{
    FundStubHit, StockStubHit, batch_body, body_to_bytes, get_json, post_batch, post_instrument,
    setup_app_with_fund_stub, setup_app_with_stock_stub,
};

fn generic_chain_stock_hits() -> HashMap<String, StockStubHit> {
    HashMap::from([
        (
            "sh/600519".to_string(),
            StockStubHit {
                name: "贵州茅台",
                price: Some((150000, "2026-09-04")),
                kind_hint: InstrumentType::Stock,
                market: Market::Sh,
            },
        ),
        (
            "sh/600036".to_string(),
            StockStubHit {
                name: "招商银行",
                price: Some((45000, "2026-09-04")),
                kind_hint: InstrumentType::Stock,
                market: Market::Sh,
            },
        ),
    ])
}

fn trade_row(
    kind: &str,
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    date: &str,
) -> String {
    format!(
        r#"{{"kind":"{kind}","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"{date}","instrument_id":"{instrument_id}","quantity":{qty},"price_cents":{price},"fee_cents":{fee}}}"#
    )
}

#[tokio::test]
async fn test_migration_chain_create_instrument_batch_import_rows_readback() {
    let (app, conn, _calls) = setup_app_with_stock_stub(generic_chain_stock_hits());

    let create_body = r#"{"symbol":"600519","type":"stock","name":"贵州茅台","market":"sh"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    let account_id = test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-inv-297",
        "证券账户",
        "investment",
        "CNY",
        0,
    );
    let rows = [
        trade_row(
            "buy",
            &account_id,
            &instrument_id,
            100.0,
            150000,
            500,
            "2026-05-01",
        ),
        trade_row(
            "buy",
            &account_id,
            &instrument_id,
            100.0,
            180000,
            100,
            "2026-05-10",
        ),
        trade_row(
            "sell",
            &account_id,
            &instrument_id,
            150.0,
            200000,
            200,
            "2026-05-20",
        ),
    ];
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let imported = post_batch(&app, batch_body(&refs, None)).await;
    assert_eq!(imported.len(), 3);
    assert!(
        imported.iter().all(|r| r["success"] == true),
        "buy/sell 行应全部成功: {imported:?}"
    );

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 3, "buy×2 + sell×1 应全部落库");
}

#[tokio::test]
async fn test_update_trade_to_missing_instrument_returns_400_not_500() {
    let (app, conn, _calls) = setup_app_with_stock_stub(generic_chain_stock_hits());

    let create_body = r#"{"symbol":"600036","type":"stock","name":"招商银行","market":"sh"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).unwrap();

    let account_id = test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-inv-297",
        "证券账户",
        "investment",
        "CNY",
        0,
    );
    let buy = trade_row(
        "buy",
        &account_id,
        &instrument_id,
        10.0,
        250000,
        0,
        "2026-05-01",
    );
    let refs = [buy.as_str()];
    let created = post_batch(&app, batch_body(&refs, None)).await;
    assert_eq!(created[0]["success"], true, "买入应成功: {:?}", created[0]);
    let txn_id = created[0]["id"].as_str().unwrap().to_string();

    let bad_body = trade_row(
        "buy",
        &account_id,
        "inst-not-exist",
        10.0,
        250000,
        0,
        "2026-05-01",
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/api/v1/transactions/{txn_id}"))
                .header("content-type", "application/json")
                .body(Body::from(bad_body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "标的不存在应 400（prepare 拦截），而非外键违规的 500"
    );
    let bytes = body_to_bytes(response.into_body()).await;
    let err: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        err["message"].as_str().unwrap().contains("买入标的不存在"),
        "错误信息应可读回自纠，实际: {err}"
    );
}

#[tokio::test]
async fn test_fund_migration_chain_lookup_create_batch_import_wired() {
    let hits = HashMap::from([(
        "012345".to_string(),
        FundStubHit {
            name: "华夏成长混合",
            fund_class: "混合型-灵活",
            nav: Some((1.65, "2026-06-30")),
            money_fund: false,
        },
    )]);
    let (app, conn, calls) = setup_app_with_fund_stub(hits);

    let (status, _) = get_json(&app, "/api/v1/funds/012345").await;
    assert_eq!(status, StatusCode::OK);

    let create_body = r#"{"symbol":"012345","type":"fund","name":"华夏成长混合(账单抄写)"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    let account_id = test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-inv-297",
        "证券账户",
        "investment",
        "CNY",
        0,
    );
    let buy = format!(
        r#"{{"kind":"buy","amount_cents":151500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-11","instrument_id":"{instrument_id}","quantity":1000,"fee_cents":1500,"idempotency_key":"fund-bill.csv:3:1"}}"#
    );
    let sell = format!(
        r#"{{"kind":"sell","amount_cents":65500,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-25","instrument_id":"{instrument_id}","quantity":400,"fee_cents":500,"idempotency_key":"fund-bill.csv:5:1"}}"#
    );
    let refs = [buy.as_str(), sell.as_str()];
    let imported = post_batch(&app, batch_body(&refs, None)).await;
    assert_eq!(imported.len(), 2);
    assert!(
        imported.iter().all(|r| r["success"] == true),
        "申购/赎回应全部成功: {imported:?}"
    );

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 2, "两行申赎应全部落库");

    assert_eq!(
        *calls.lock().unwrap(),
        vec!["012345".to_string(), "012345".to_string()]
    );
}

#[tokio::test]
async fn test_stock_migration_chain_lookup_create_batch_import_wired() {
    let hits = HashMap::from([(
        "sh/600519".to_string(),
        StockStubHit {
            name: "贵州茅台",
            price: Some((200000, "2026-09-04")),
            kind_hint: InstrumentType::Stock,
            market: Market::Sh,
        },
    )]);
    let (app, conn, calls) = setup_app_with_stock_stub(hits);

    let (status, _) = get_json(&app, "/api/v1/stocks/600519").await;
    assert_eq!(status, StatusCode::OK);

    let create_body =
        r#"{"symbol":"600519","type":"stock","market":"sh","name":"贵州茅台(账单抄写)"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    let account_id = test_support::seed_account(
        &conn.lock().unwrap(),
        "acc-inv-297",
        "证券账户",
        "investment",
        "CNY",
        0,
    );
    let buy = format!(
        r#"{{"kind":"buy","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-11","instrument_id":"{instrument_id}","quantity":100,"price_cents":150000,"fee_cents":500,"idempotency_key":"stock-bill.csv:3:1"}}"#
    );
    let sell = format!(
        r#"{{"kind":"sell","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"2026-05-25","instrument_id":"{instrument_id}","quantity":40,"price_cents":200000,"fee_cents":200,"idempotency_key":"stock-bill.csv:5:1"}}"#
    );
    let refs = [buy.as_str(), sell.as_str()];
    let imported = post_batch(&app, batch_body(&refs, None)).await;
    assert_eq!(imported.len(), 2);
    assert!(
        imported.iter().all(|r| r["success"] == true),
        "买入/卖出行应全部成功: {imported:?}"
    );

    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 2, "两行买卖应全部落库");

    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("sh".to_string(), "600519".to_string()),
            ("sh".to_string(), "600519".to_string())
        ]
    );
}
