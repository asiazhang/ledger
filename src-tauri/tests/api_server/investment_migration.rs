//! 投资迁移链路 API 集成测试——端点壳三件套 + 接线证明（issue #773 / ADR-0087 决策 5）。
//!
//! 只保留壳语义与接线证明（ADR-0087 决策 2：一步可观察——请求经接口进入写入
//! 接缝并产生直接可观察结果，存在性的出现属证明范围）：HTTP 建标的 → 批量导入
//! buy/sell → 读回行存在；含「标的不存在」更新路径的错误形态断言（400 非 500，
//! issue #295 prepare 拦截的对外形状）。金额折算、FIFO、持仓、盈亏、余额数值等
//! 域语义权威在域单测（`investment/tests/trade.rs`、`pnl.rs`、`fund_trade.rs`，
//! `transaction/tests/amount.rs`、`balance_cache.rs`）；链路「搜索→建标的→批量
//! 导入→读回」的旅程权威在 e2e BDD（`e2e/features/instruments.feature` 投资迁移链路）。
//!
//! 通用链路示例标的用非 fund 类型（stock，东财往返经注入桩离线驱动——issue #694
//! 起 stock 真实代码创建经东财增强）；基金申赎迁移链路（查询→创建→
//! 批量导入，issue #304）与股票三步法链路（issue #694）见本文件末尾独立测试。

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tauri_app_lib::investment::InstrumentType;
use tauri_app_lib::test_support;
use tower::ServiceExt;

use crate::common::{
    FundStubHit, StockStubHit, batch_body, body_to_bytes, get_json, post_batch, post_instrument,
    setup_app_with_fund_stub, setup_app_with_stock_stub,
};

/// 通用链路的股票东财桩命中表（issue #694 起 stock 真实代码创建经东财增强，
/// 全部链路测试离线驱动、不触真实网络）。
fn generic_chain_stock_hits() -> HashMap<String, StockStubHit> {
    HashMap::from([
        (
            "sh/600519".to_string(),
            StockStubHit {
                name: "贵州茅台",
                price: Some((150000, "2026-09-04")),
                kind_hint: InstrumentType::Stock,
            },
        ),
        (
            "sh/600036".to_string(),
            StockStubHit {
                name: "招商银行",
                price: Some((45000, "2026-09-04")),
                kind_hint: InstrumentType::Stock,
            },
        ),
    ])
}

/// 批量导入一笔 buy/sell 行（金额占位 0：交易行金额由行为层 prepare 按数量×单价±手续费重算）。
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

/// 通用链路接线证明（ADR-0087 决策 2）：HTTP 建标的 → 批量导入 buy/sell →
/// 读回行存在。行金额折算、FIFO 消耗、持仓与余额数值不在此展开——域语义权威
/// 坐标见文件头；幂等创建、搜索、批量壳形态另有专项 api_server 文件覆盖。
#[tokio::test]
async fn test_migration_chain_create_instrument_batch_import_rows_readback() {
    let (app, conn, _calls) = setup_app_with_stock_stub(generic_chain_stock_hits());

    // HTTP 建标的（东财桩离线增强）→ 201 + 裸 id
    let create_body = r#"{"symbol":"600519","type":"stock","name":"贵州茅台","market":"sh"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    // 批量导入 buy/sell（金额占位 0，由行为层 prepare 重算）→ 行全部成功
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

    // 接线证明（存在性）：经端点导入的行可直接读回
    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 3, "buy×2 + sell×1 应全部落库");
}

/// 「标的不存在」更新路径的错误形态断言（壳三件套）：把已导入的买入修改为引用
/// 不存在标的，行为层 prepare 拦截（issue #295）上抛统一错误形状的中文 400，非 500。
/// 原交易保持不变等域语义权威在 `investment/tests/trade.rs`
/// `update_buy_to_missing_instrument_rejected_and_keeps_original`。
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

    // 修改为引用不存在标的的买入：400 + 统一错误形状（kind/message 中文），非 500
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

// ---------------------------------------------------------------------------
// 基金申赎迁移链路（issue #304 / ADR-0039）：查询 → 创建 → 批量导入的
// 接线证明（东财经注入桩离线驱动）。确认单金额权威、净值反算、幂等键去重、
// 余额口径等域语义权威见文件头坐标，此处不展开。
// ---------------------------------------------------------------------------

/// 基金链路接线证明（ADR-0087 决策 2）：HTTP 查询 → 创建 → 批量导入 → 读回行存在；
/// 桩注入装配证明全链路对东财的依赖仅两次（查询 + 创建校验），批量导入零网络。
#[tokio::test]
async fn test_fund_migration_chain_lookup_create_batch_import_wired() {
    let hits = HashMap::from([(
        "012345".to_string(),
        FundStubHit {
            name: "华夏成长混合",
            fund_class: "混合型-灵活",
            nav: Some((1.65, "2026-06-30")),
        },
    )]);
    let (app, conn, calls) = setup_app_with_fund_stub(hits);

    // 1. 按代码查询（链路起点；投影字段壳权威在 fund_lookup.rs）
    let (status, _) = get_json(&app, "/api/v1/funds/012345").await;
    assert_eq!(status, StatusCode::OK);

    // 2. 以真实 6 位代码创建标的 → 201 + 裸 id（东财回填细节权威在
    //    instrument_create_fund.rs）
    let create_body = r#"{"symbol":"012345","type":"fund","name":"华夏成长混合(账单抄写)"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    // 3. 批量提交申购/赎回（确认单金额权威、不传单价，issue #302 / ADR-0038；
    //    幂等键取源内稳定行号）→ 行全部成功
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

    // 4. 接线证明（存在性）：经端点导入的行可直接读回
    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 2, "两行申赎应全部落库");

    // 桩注入装配：全链路对东财的依赖仅两次（查询 + 创建校验），批量导入零网络
    assert_eq!(
        *calls.lock().unwrap(),
        vec!["012345".to_string(), "012345".to_string()]
    );
}

// ---------------------------------------------------------------------------
// 股票迁移链路（issue #694 / ADR-0081）：查询 → 创建（东财增强）→ 批量导入的
// 接线证明，与基金申赎链路对称；空标的字典账本起点，全程不依赖全量同步。
// ---------------------------------------------------------------------------

/// 股票链路接线证明（ADR-0087 决策 2）：HTTP 查询 → 创建 → 批量导入 → 读回行存在；
/// 桩注入装配证明全链路对东财的依赖仅两次（查询 + 创建校验），批量导入零网络。
#[tokio::test]
async fn test_stock_migration_chain_lookup_create_batch_import_wired() {
    let hits = HashMap::from([(
        "sh/600519".to_string(),
        StockStubHit {
            name: "贵州茅台",
            price: Some((200000, "2026-09-04")),
            kind_hint: InstrumentType::Stock,
        },
    )]);
    let (app, conn, calls) = setup_app_with_stock_stub(hits);

    // 1. 先按代码查询（链路起点；投影字段壳权威在 stock_lookup.rs）
    let (status, _) = get_json(&app, "/api/v1/stocks/600519").await;
    assert_eq!(status, StatusCode::OK);

    // 2. 再以真实代码 + 精确市场创建标的 → 201 + 裸 id（东财增强回填细节权威在
    //    instrument_create_stock.rs）
    let create_body =
        r#"{"symbol":"600519","type":"stock","market":"sh","name":"贵州茅台(账单抄写)"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    // 3. 批量提交 buy/sell（数量 × 单价权威、金额由服务端重算；幂等键取源内稳定行号）
    //    → 行全部成功
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

    // 4. 接线证明（存在性）：经端点导入的行可直接读回
    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 2, "两行买卖应全部落库");

    // 桩注入装配：全链路对东财的依赖仅两次（查询 + 创建校验），批量导入零网络
    assert_eq!(
        *calls.lock().unwrap(),
        vec![
            ("sh".to_string(), "600519".to_string()),
            ("sh".to_string(), "600519".to_string())
        ]
    );
}
