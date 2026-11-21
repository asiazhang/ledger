use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use ledger_dashboard as dashboard;
use ledger_investment::{Holding, PnlFilter, list_holdings, query_realized_pnl_summary};
use rusqlite::Connection;
use tauri_app_lib::test_support;

use crate::common::{
    FundStubHit, batch_body, get_json, post_batch, post_instrument, setup_app_with_fund_stub,
};

struct SplitRecord {
    delta: f64,
    date: &'static str,
    source_key: &'static str,
}

const SPLIT_RECORDS: [SplitRecord; 6] = [
    SplitRecord {
        delta: 153.12,
        date: "2019-07-08",
        source_key: "qieman-2019-07.csv:144:1",
    },
    SplitRecord {
        delta: 36.15,
        date: "2019-07-08",
        source_key: "qieman-2019-07.csv:144:2",
    },
    SplitRecord {
        delta: 119.20,
        date: "2020-07-08",
        source_key: "qieman-2020-07.csv:144:1",
    },
    SplitRecord {
        delta: 28.14,
        date: "2020-07-08",
        source_key: "qieman-2020-07.csv:144:2",
    },
    SplitRecord {
        delta: 0.60,
        date: "2020-07-08",
        source_key: "qieman-2020-07.csv:144:3",
    },
    SplitRecord {
        delta: 2.55,
        date: "2020-07-08",
        source_key: "qieman-2020-07.csv:144:4",
    },
];

const QUANTITY_BEFORE: f64 = 5_480.77;
const QUANTITY_AFTER: f64 = 5_820.53;
const BLOCKED_SELL_QUANTITY: f64 = 5_820.53;
const BLOCKED_SELL_AMOUNT_CENTS: i64 = 773_025;
const NAV_PRICE_CENTS: i64 = 13_281;
const BUY_AMOUNT_CENTS: i64 = 548_077;

fn split_row(account_id: &str, instrument_id: &str, delta: f64, date: &str, key: &str) -> String {
    format!(
        r#"{{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"{date}","instrument_id":"{instrument_id}","quantity":{delta},"idempotency_key":"{key}"}}"#
    )
}

fn fund_sell_row(account_id: &str, instrument_id: &str, quantity: f64, date: &str) -> String {
    format!(
        r#"{{"kind":"sell","amount_cents":{BLOCKED_SELL_AMOUNT_CENTS},"currency_code":"CNY","account_id":"{account_id}","date":"{date}","instrument_id":"{instrument_id}","quantity":{quantity},"fee_cents":0}}"#
    )
}

struct AcceptanceReadout {
    holding: Holding,
    net_worth_cents: i64,
    realized_pnl_cents: i64,
}

fn holding_of(conn: &Arc<Mutex<Connection>>, instrument_id: &str) -> Option<Holding> {
    list_holdings(&conn.lock().unwrap())
        .unwrap()
        .into_iter()
        .find(|h| h.instrument_id == instrument_id)
}

fn net_worth_of(conn: &Arc<Mutex<Connection>>) -> i64 {
    dashboard::query_dashboard_overview(&conn.lock().unwrap())
        .unwrap()
        .net_worth_cents
}

fn realized_of(conn: &Arc<Mutex<Connection>>, account_id: &str, instrument_id: &str) -> i64 {
    query_realized_pnl_summary(
        &conn.lock().unwrap(),
        &PnlFilter {
            account_id: Some(account_id.to_string()),
            instrument_id: Some(instrument_id.to_string()),
        },
    )
    .unwrap()
    .by_year
    .iter()
    .map(|r| r.realized_pnl_cents.unwrap_or(0))
    .sum()
}

fn readout(
    conn: &Arc<Mutex<Connection>>,
    account_id: &str,
    instrument_id: &str,
) -> AcceptanceReadout {
    AcceptanceReadout {
        holding: holding_of(conn, instrument_id).expect("502010 持仓视图行应存在"),
        net_worth_cents: net_worth_of(conn),
        realized_pnl_cents: realized_of(conn, account_id, instrument_id),
    }
}

#[tokio::test]
async fn test_split_real_migration_data_unblocks_blocked_sell_end_to_end() {
    let hits = HashMap::from([(
        "502010".to_string(),
        FundStubHit {
            name: "易方达中证全指证券公司指数(LOF)A",
            fund_class: "指数型-股票",
            nav: Some((1.3281, "2026-06-26")),
            money_fund: false,
        },
    )]);
    let (app, conn, _calls) = setup_app_with_fund_stub(hits);

    let (status, bytes) = post_instrument(
        &app,
        r#"{"symbol":"502010","type":"fund","name":"易方达中证全指证券公司指数(LOF)A"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let instrument_id: String = serde_json::from_slice(&bytes).expect("201 应为裸 id 字符串");

    let account_id = {
        let conn = conn.lock().unwrap();
        test_support::seed_account(&conn, "acc-qieman", "且慢", "investment", "CNY", 0)
    };

    let buy = format!(
        r#"{{"kind":"buy","amount_cents":{BUY_AMOUNT_CENTS},"currency_code":"CNY","account_id":"{account_id}","date":"2018-12-20","instrument_id":"{instrument_id}","quantity":{QUANTITY_BEFORE},"fee_cents":0}}"#
    );
    let created = post_batch(&app, batch_body(&[buy.as_str()], None)).await;
    assert_eq!(created[0]["success"], true, "建仓应成功: {created:?}");

    let sell = fund_sell_row(
        &account_id,
        &instrument_id,
        BLOCKED_SELL_QUANTITY,
        "2026-06-26",
    );
    let blocked = post_batch(&app, batch_body(&[sell.as_str()], None)).await;
    assert_eq!(
        blocked[0]["success"], false,
        "份额结转缺失时应被可卖出数量守卫拒绝: {blocked:?}"
    );
    assert!(
        blocked[0]["error"]
            .as_str()
            .unwrap_or("")
            .contains("可卖出数量不足"),
        "阻塞原因应是可卖出数量不足，实际: {blocked:?}"
    );

    let (_, balances_before) = get_json(&app, "/api/v1/accounts/balances").await;
    let before = readout(&conn, &account_id, &instrument_id);
    assert!(
        (before.holding.quantity - QUANTITY_BEFORE).abs() < 1e-6,
        "建仓后持仓应为 5,480.77，实际 {}",
        before.holding.quantity
    );

    let declared_increment: f64 = SPLIT_RECORDS.iter().map(|r| r.delta).sum();
    assert!(
        (declared_increment - 339.76).abs() < 1e-9,
        "6 笔结转合计应为 +339.76（且慢快照事实），实际 {declared_increment}"
    );
    let rows: Vec<String> = SPLIT_RECORDS
        .iter()
        .map(|r| split_row(&account_id, &instrument_id, r.delta, r.date, r.source_key))
        .collect();
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    let imported = post_batch(&app, batch_body(&refs, None)).await;
    assert_eq!(imported.len(), 6, "6 笔结转应逐行返回结果: {imported:?}");
    assert!(
        imported.iter().all(|r| r["success"] == true),
        "6 笔份额结转应全部落账: {imported:?}"
    );

    let after = readout(&conn, &account_id, &instrument_id);
    assert!(
        (after.holding.quantity - QUANTITY_AFTER).abs() < 1e-6,
        "结转后持仓应为 5,820.53，实际 {}",
        after.holding.quantity
    );

    let (_, balances_after) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(
        balances_before, balances_after,
        "份额调整落账前后全部账户余额应完全不变"
    );

    let turnover = after.holding.quantity - before.holding.quantity;
    assert!(
        (turnover - declared_increment).abs() < 1e-6,
        "持仓视图前后差应等于 6 笔结转 Δ 合计 +{declared_increment}（份额换手），实际 {turnover}"
    );

    let market_delta = after
        .holding
        .market_value_cents
        .expect("现价在，市值应有值")
        - before
            .holding
            .market_value_cents
            .expect("现价在，市值应有值");
    let share_increment_value = (turnover * NAV_PRICE_CENTS as f64 / 100.0).round() as i64;
    assert_eq!(
        market_delta, 45_124,
        "市值增量 = ROUND(5,820.53×1.3281) − ROUND(5,480.77×1.3281)"
    );
    assert_eq!(
        market_delta, share_increment_value,
        "市值增量应等于份额增量 × 现价（万分位刻度）"
    );
    assert_eq!(
        after.net_worth_cents - before.net_worth_cents,
        market_delta,
        "净资产随持仓市值同步变化（本场景无现金腿与非投资账户）"
    );
    assert_eq!(before.holding.market_value_cents, Some(727_901));
    assert_eq!(after.holding.market_value_cents, Some(773_025));
    assert_eq!(before.holding.unrealized_pnl_cents, Some(179_824));
    assert_eq!(after.holding.unrealized_pnl_cents, Some(224_964));
    assert_eq!(
        after
            .holding
            .unrealized_pnl_cents
            .expect("现价在，未实现盈亏应有值")
            - before
                .holding
                .unrealized_pnl_cents
                .expect("现价在，未实现盈亏应有值"),
        45_140,
        "未实现盈亏增量 = 份额增量市值 45,124 + 成本展示舍入 16"
    );
    assert_eq!(after.realized_pnl_cents, 0, "份额调整恒不产生已实现盈亏");
    assert_eq!(
        before.realized_pnl_cents, after.realized_pnl_cents,
        "份额调整前后已实现盈亏不变"
    );

    let (_, list) = get_json(&app, "/api/v1/transactions?kinds=split").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 6, "6 笔结转应逐条可读回");
    assert!(
        items
            .iter()
            .all(|t| t["kind"] == "split" && t["amount_cents"] == 0),
        "split 行金额应恒为 0: {items:?}"
    );

    let sold = post_batch(&app, batch_body(&[sell.as_str()], None)).await;
    assert_eq!(
        sold[0]["success"], true,
        "份额补足后真实卖出应走通: {sold:?}"
    );
    assert!(
        holding_of(&conn, &instrument_id).is_none(),
        "真实卖出为全额清仓，持仓视图行应消失"
    );
    assert_eq!(
        realized_of(&conn, &account_id, &instrument_id),
        224_948,
        "Σ已实现 = Σ卖出(773,025) − Σ买入(548,077) 精确闭合"
    );
}
