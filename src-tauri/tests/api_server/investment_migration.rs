//! 投资迁移全链路 HTTP 集成测试（issue #297 / ADR-0037）。
//!
//! 端到端固化 AI 投资迁移链路并钉住读回口径：搜索标的（无命中）→ 幂等创建 →
//! 批量导入 buy/sell → 持仓批次正确（买入建仓、卖出 FIFO 跨批次消耗、手续费
//! 按数量分摊、已实现盈亏）→ 读回核对（交易行金额 = 数量 × 单价 ± 手续费）→
//! 余额口径核对（投资账户现金流：buy 含费流出、sell 净额流入）。
//! 「标的不存在」路径断言为 400（非 500，issue #295 prepare 拦截的对外形状）。
//!
//! 链路示例标的用非 fund 类型（stock）——fund 创建行为将由 #304 东财校验收紧，
//! 避免跨票翻红（ADR-0037 修订记录③）。

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use rusqlite::params;
use tower::ServiceExt;

use crate::common::{batch_body, body_to_bytes, get_json, post_batch, setup_app};

/// POST /api/v1/instruments，返回（状态码，原始响应体）。响应体不在此处反序列化：
/// 201 为裸 id 字符串，由调用方自行解析（先例 instrument_create.rs）。
async fn post_instrument(app: &Router, body: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/instruments")
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

/// 直插投资账户（账户创建 API 的夹具固定 cash 类型；先例 batch_import.rs #295 测试）。
fn seed_investment_account(conn: &Arc<Mutex<rusqlite::Connection>>) -> String {
    conn.lock().unwrap().execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('acc-inv-297','证券账户','investment','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
    ).unwrap();
    "acc-inv-297".to_string()
}

/// 锚定持仓批次顺序：`now_iso` 精度为秒，同一批次内连续落库的两笔 buy 其批次
/// `created_at` 相同，FIFO 排序将退化为 uuid 随机序——按买入交易 id 回填错开的
/// `created_at` 保证确定性（先例：投资域单测 tests/trade.rs 同款夹具）。
fn anchor_lot_order(conn: &Arc<Mutex<rusqlite::Connection>>, buy_txn_id: &str, created_at: &str) {
    conn.lock()
        .unwrap()
        .execute(
            "UPDATE security_lots SET created_at=?1 WHERE buy_transaction_id=?2",
            params![created_at, buy_txn_id],
        )
        .unwrap();
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

/// 链路数字（buy 含费建仓、sell 减费回款，全部整除便于断言；单价为万分之一元刻度）：
/// - buy1: 100 × 150000/100 + 500  = 150500 → lot1 每份成本 150500（15.05 元）
/// - buy2: 100 × 180000/100 + 100  = 180100 → lot2 每份成本 180100（18.01 元）
/// - sell: 150 × 2000 − 200  = 299800；FIFO 消耗 lot1 全部 100 + lot2 一半 50
///   - lot1 匹配：费用分摊 floor(200×100/150)=133，盈亏 200000−150500−133 = 49367
///   - lot2 匹配：费用吃余 200−133=67，盈亏 100000−90050−67 = 9883
///   - 已实现盈亏合计 59250；剩余持仓 lot2 的 50 份
/// - 余额：0 − 150500 − 180100 + 299800 = −30800（现金流口径）
#[tokio::test]
async fn test_full_migration_flow_search_create_buy_sell_holdings_balance() {
    let (app, conn) = setup_app();

    // 1. 搜索（链路起点）：无命中
    let (status, body) = get_json(&app, "/api/v1/instruments?query=600519").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0, "链路起点应无命中");

    // 2. 幂等创建（POST /api/v1/instruments）：201 + 裸 id；重放同自然键复用同一 id
    let create_body = r#"{"symbol":"600519","type":"stock","name":"贵州茅台","market":"sh"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");
    let (replay_status, replay_bytes) = post_instrument(&app, create_body).await;
    assert_eq!(replay_status, StatusCode::CREATED);
    let replay_id: String = serde_json::from_slice(&replay_bytes).unwrap();
    assert_eq!(replay_id, instrument_id, "链路重跑应幂等复用同一标的");

    // 3. 批量导入第一批：两笔买入建仓（不同价格批次，含手续费）
    let account_id = seed_investment_account(&conn);
    let buys = [
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
    ];
    let refs: Vec<&str> = buys.iter().map(String::as_str).collect();
    let first = post_batch(&app, batch_body(&refs, None)).await;
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|r| r["success"] == true && r["duplicate"] == false),
        "买入行应全部成功: {first:?}"
    );
    let buy1_id = first[0]["id"].as_str().unwrap().to_string();
    let buy2_id = first[1]["id"].as_str().unwrap().to_string();

    // 锚定批次顺序后，第二批导入跨批次卖出（同批 buy/sell 会让 FIFO 匹配先于锚定发生）
    anchor_lot_order(&conn, &buy1_id, "2026-05-01T00:00:00Z");
    anchor_lot_order(&conn, &buy2_id, "2026-05-02T00:00:00Z");
    let sells = [trade_row(
        "sell",
        &account_id,
        &instrument_id,
        150.0,
        200000,
        200,
        "2026-05-20",
    )];
    let refs: Vec<&str> = sells.iter().map(String::as_str).collect();
    let second = post_batch(&app, batch_body(&refs, None)).await;
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["success"], true, "卖出行应成功: {:?}", second[0]);

    // 4. 读回核对：交易行金额 = 数量 × 单价 + 手续费（buy）/ − 手续费（sell）
    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 3, "buy×2 + sell×1 应全部落库");
    let amounts: Vec<i64> = items
        .iter()
        .map(|t| t["amount_cents"].as_i64().unwrap())
        .collect();
    assert_eq!(
        amounts
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        [150500i64, 180100, 299800].into_iter().collect(),
        "交易行金额应为 数量×单价±手续费（占位 0 应被后端重算覆盖）"
    );

    // 5. 持仓批次核对：买入建仓（每份成本含费均摊）、卖出 FIFO 消耗（块作用域内
    //    完成全部同步查询，MutexGuard 不跨 await）
    {
        let conn = conn.lock().unwrap();
        let lot_of = |buy_txn: &str| -> (f64, i64) {
            conn.query_row(
                "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
                params![buy_txn],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ).unwrap()
        };
        let (rem1, cost1) = lot_of(&buy1_id);
        let (rem2, cost2) = lot_of(&buy2_id);
        assert!((rem1 - 0.0).abs() < 1e-9, "先建批次应被 FIFO 全部消耗");
        assert!((rem2 - 50.0).abs() < 1e-9, "后建批次应剩余 50 份");
        assert_eq!(
            cost1, 150500,
            "批次每份成本（万分之一元）= (数量×单价+手续费×100)/数量"
        );
        assert_eq!(cost2, 180100);

        let mut sales: Vec<(f64, i64, i64)> = conn
            .prepare(
                "SELECT sls.quantity, sls.cost_per_unit_cents, sls.realized_pnl_cents \
             FROM security_lot_sales sls \
             JOIN security_lots l ON l.id = sls.lot_id \
             WHERE l.buy_transaction_id IN (?1, ?2) ORDER BY l.created_at ASC, l.id ASC",
            )
            .unwrap()
            .query_map(params![buy1_id, buy2_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(sales.len(), 2, "跨两批次的卖出应产生两条匹配记录");
        // FIFO：先匹配先建批次（手续费按数量比例分摊，末批吃余）
        let first_match = sales.remove(0);
        assert_eq!(
            first_match,
            (100.0, 150500, 49367),
            "先建批次匹配行应为全量 100 份"
        );
        let second_match = sales.remove(0);
        assert_eq!(
            second_match,
            (50.0, 180100, 9883),
            "后建批次匹配行应为余量 50 份"
        );
    }

    // 6. 余额口径核对：投资账户现金流 = 初始 − Σ买入含费 + Σ卖出净额
    let (_, balances) = get_json(&app, "/api/v1/accounts/balances").await;
    let rows = balances.as_array().unwrap();
    let securities = rows
        .iter()
        .find(|r| r["account"]["name"] == "证券账户")
        .expect("余额清单应含投资账户");
    assert_eq!(
        securities["balance_cents"], -30800,
        "投资账户余额应为现金流口径（buy 含费流出、sell 净额流入）"
    );
}

/// 「标的不存在」路径在链路中断言为 400（非 500）：把已导入的买入修改为引用
/// 不存在标的，行为层 prepare 拦截（issue #295）上抛统一错误形状的中文 400，
/// 原交易保持不变——AI 可据此回自纠（重搜/重建标的后再提交）。
#[tokio::test]
async fn test_update_trade_to_missing_instrument_returns_400_not_500() {
    let (app, conn) = setup_app();

    let create_body = r#"{"symbol":"600036","type":"stock","name":"招商银行","market":"sh"}"#;
    let (status, bytes) = post_instrument(&app, create_body).await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).unwrap();

    let account_id = seed_investment_account(&conn);
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

    // 原交易保持不变（金额与买卖明细均未被动过）
    let (_, list) = get_json(&app, "/api/v1/transactions").await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], txn_id);
    assert_eq!(
        items[0]["amount_cents"], 25000,
        "10 × 25.00 元 + 0，原金额不变"
    );
    let lot_count: i64 = conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE buy_transaction_id=?1",
            params![txn_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lot_count, 1, "原持仓批次不应被误清理");
}
