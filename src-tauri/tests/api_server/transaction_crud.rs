use axum::http::StatusCode;

use tauri_app_lib::test_support;

use crate::common::{
    batch_body, create_account_via_api, delete_account_via_api, delete_transaction_via_api,
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

    // 读回应反映修改。
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

/// issue #295：修改（全字段替换）把买入改为引用不存在的标的 → 400 Invalid 中文
/// 错误（此前为扩展表外键违规的 500 数据库错误），原交易行与持仓批次保持不变。
#[tokio::test]
async fn test_update_buy_to_missing_instrument_returns_400_with_readable_error() {
    let (app, conn) = setup_app();
    // 投资铺垫（账户+标的+1:1 汇率）一行建成：工厂组合种子（spec #728 / ADR-0084）。
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-295", "inst-295");

    let buy = r#"{"transactions":[{"kind":"buy","amount_cents":0,"currency_code":"USD","account_id":"acc-inv-295","date":"2026-01-10","instrument_id":"inst-295","quantity":10.0,"price_cents":1000000,"fee_cents":0}]}"#;
    let created = post_batch(&app, buy.to_string()).await;
    assert_eq!(created[0]["success"], true, "铺垫买入应成功");
    let id = created[0]["id"].as_str().unwrap().to_string();

    // 改为引用不存在的标的 → 400（非 500），错误可读、携带标的 id。
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

    // 原交易行与持仓批次保持原样（入口自持事务整体回滚）。
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

// ---------------------------------------------------------------------------
// sell 创建端点壳层接线证明（ADR-0087 决策 2）：仅证请求经批量端点进入写入
// 接缝且资源可读；卖出金额公式与本位币折算等域结果细节归域单测
// （investment/tests/trade.rs、transaction/tests/amount.rs）。
// ---------------------------------------------------------------------------

/// sell 经批量端点进入写入接缝的接线证明（issue #771）：创建成功且资源可读，
/// 不展开折算数值等域结果细节。
#[tokio::test]
async fn test_create_sell_via_batch_succeeds_and_readable() {
    let (app, conn) = setup_app();
    // 投资铺垫（账户+标的+1:1 汇率）一行建成：工厂组合种子（spec #728 / ADR-0084）。
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-sell", "inst-sell");

    // 前置：买入建仓（卖出按 FIFO 消费持仓），经同一公开端点造数。
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

    // 接线证明：创建的资源可读回。
    let sell_id = results[0]["id"].as_str().unwrap();
    let (_, readback) = get_json(&app, "/api/v1/transactions").await;
    let txs = items_of(&readback);
    let row = txs
        .iter()
        .find(|t| t["id"].as_str() == Some(sell_id))
        .expect("创建的卖出应可读回");
    assert_eq!(row["kind"], "sell");
}

// ---------------------------------------------------------------------------
// 出资账户（issue #935 / ADR-0096）：壳层接线证明。参数解包、状态码与错误码
// 在此锁定；准入闭集与归因语义的域细节归域单测
//（transaction/funding/tests.rs、transaction/tests/amount.rs）。
// ---------------------------------------------------------------------------

/// 买入携带出资账户：参数解包 + 读回形状证明。创建成功且读回携带出资账户 id。
#[tokio::test]
async fn test_create_buy_with_funding_account_roundtrips_readback() {
    let (app, conn) = setup_app();
    test_support::seed_investment_setup(&conn.lock().unwrap(), "acc-inv-fund", "inst-fund");
    // USD 现金出资账户（与投资账户同币种，现金类准入闭集内）。
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

    // 读回携带出资账户（可选字段出现在读模型）。
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

/// 通用 kind 携带出资账户：批量端点行级容错（Invalid 类归行级 success:false）。
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

/// 投资账户不可作出资账户（准入闭集排除 investment）：顶层码化 400。
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

/// 出资账户币种与结算币种不一致：顶层码化 400，params 按消息动态值顺序排列。
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

/// 软删出资账户不可被新选择：顶层码化 404（存在且未软删校验，与商户/保单同款）。
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
