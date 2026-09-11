//! 份额调整（split）真实数据端到端验收（spec #1045 / ADR-0106 决策 12 / issue #1055）。
//!
//! 以迁移侧且慢快照的真实事件为锚，经**唯一写入面**（HTTP 批量端点）实写 replay，
//! 钉住四条用户可观察结论：
//!
//! 1. 502010（易方达中证全指证券公司指数(LOF)A）的 6 笔 `typeCode=144` 年度份额
//!    结转（+339.76 份，快照中无金额 / 无净值 / 无现金腿）落账后，持仓
//!    `5,480.77 + 339.76 = 5,820.53`，被 339.76 份缺口阻塞的真实卖出
//!    （2026-06-26，5,820.53 份 / 7,730.25 元）实写走通；
//! 2. `GET /api/v1/accounts/balances` 份额调整前后**完全不变**（无现金腿）；
//! 3. 持仓视图（`investment::list_holdings`）数量前后差 = +339.76（份额换手）；
//! 4. 净资产（`dashboard::query_dashboard_overview`）与未实现盈亏（Holding）按份额
//!    增量变化，已实现盈亏（`investment::query_realized_pnl_summary`）不变。
//!
//! 本层钉住的是**本票验收锚点**（持仓 / 换手 / 净资产 / 未实现 / 已实现盈亏）——按
//! ADR-0106 决策 12「API 集成实写本票验收」落在此层，是该决策对 ADR-0087/CONTEXT-testing
//! 「域结果细节归域单测」的**显式例外**；本测试不主张第二份域语义权威。更细的域语义
//! （重述尾差、部分卖出按重述后每份成本结算、缩股守卫、无 split 时闭合口径恒等）仍由
//! 投资域单测（`investment::tests::split`）独占权威。迁移仓闭环（快照 → 月度 CSV →
//! 份额调整落账 → 边界零未决，`asiazhang/ledger-migrate`）属**外部证据**，不进本仓
//! 测试，见 PR 描述（decision 12）。moomoo 5 笔送股不纳入本票验收（数据源待重新导出）。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use rusqlite::Connection;
use tauri_app_lib::dashboard;
use tauri_app_lib::investment::{Holding, PnlFilter, list_holdings, query_realized_pnl_summary};
use tauri_app_lib::test_support;

use crate::common::{
    FundStubHit, batch_body, get_json, post_batch, post_instrument, setup_app_with_fund_stub,
};

/// 单笔且慢 `typeCode=144` 年度份额结转（标的 502010，无金额 / 无净值）。
struct SplitRecord {
    /// 带符号份额增量 Δ（且慢 144 长赢年度结转恒为正）。
    delta: f64,
    /// 确认日（快照事实）。
    date: &'static str,
    /// 迁移侧源内稳定行的幂等键——同日多笔 `amount=0` 行的内容哈希会互相碰撞，幂等键
    /// 优先去重是逐笔落账的既有形态（内容哈希忽略 `quantity`，见
    /// `transaction::TransactionBatch`）。
    source_key: &'static str,
}

/// 且慢 `typeCode=144` 长赢年度份额结转的 6 笔真实事件：2019-07-08 两笔、2020-07-08
/// 四笔，合计 **+339.76 份**（ADR-0106 背景表）。
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

/// 结转前持仓份额（迁移侧被阻塞点）。
const QUANTITY_BEFORE: f64 = 5_480.77;
/// 结转后持仓份额 = 5,480.77 + 339.76（验收锚点）。
const QUANTITY_AFTER: f64 = 5_820.53;
/// 被阻塞的真实卖出：5,820.53 份 / 7,730.25 元（确认单金额权威，单价由后端反算）。
const BLOCKED_SELL_QUANTITY: f64 = 5_820.53;
const BLOCKED_SELL_AMOUNT_CENTS: i64 = 773_025;
/// 502010 单位净值（万分之一元）：由卖出权威金额反算 7,730.25 ÷ 5,820.53 ≈ 1.3281 元。
/// 作为「最新公布净值」种入现价，供 `v_holdings` 市值 / 未实现盈亏与净资产计算消费。
const NAV_PRICE_CENTS: i64 = 13_281;
/// 买入成本：5,480.77 份 @ 1.00 元（金额权威），成本口径不变式的独立锚点。
const BUY_AMOUNT_CENTS: i64 = 548_077;

/// 份额调整落账输入（ADR-0106 决策 1/3：单标的、带符号 Δ、无金额 / 无手续费 /
/// 无出资账户、`price_cents` 缺省）。
fn split_row(account_id: &str, instrument_id: &str, delta: f64, date: &str, key: &str) -> String {
    format!(
        r#"{{"kind":"split","amount_cents":0,"currency_code":"CNY","account_id":"{account_id}","date":"{date}","instrument_id":"{instrument_id}","quantity":{delta},"idempotency_key":"{key}"}}"#
    )
}

/// 场外基金赎回行（ADR-0038：金额权威、单价不传、由后端反算；无手续费）。
fn fund_sell_row(account_id: &str, instrument_id: &str, quantity: f64, date: &str) -> String {
    format!(
        r#"{{"kind":"sell","amount_cents":{BLOCKED_SELL_AMOUNT_CENTS},"currency_code":"CNY","account_id":"{account_id}","date":"{date}","instrument_id":"{instrument_id}","quantity":{quantity},"fee_cents":0}}"#
    )
}

/// 份额调整验收的四路可观察读数：持仓视图 + 净资产 + 未实现 / 已实现盈亏。
struct AcceptanceReadout {
    holding: Holding,
    net_worth_cents: i64,
    realized_pnl_cents: i64,
}

/// 读某持仓视图行（`remaining_quantity > 0` 才在视图内，清仓后为 `None`）。
fn holding_of(conn: &Arc<Mutex<Connection>>, instrument_id: &str) -> Option<Holding> {
    list_holdings(&conn.lock().unwrap())
        .unwrap()
        .into_iter()
        .find(|h| h.instrument_id == instrument_id)
}

/// 读本位币净资产合计（`dashboard` 域公开读口径）。
fn net_worth_of(conn: &Arc<Mutex<Connection>>) -> i64 {
    dashboard::query_dashboard_overview(&conn.lock().unwrap())
        .unwrap()
        .net_worth_cents
}

/// 读某标的的已实现盈亏合计（按匹配行币种分组后求和，与投资域读口径同源）。
fn realized_of(conn: &Arc<Mutex<Connection>>, account_id: &str, instrument_id: &str) -> i64 {
    query_realized_pnl_summary(
        &conn.lock().unwrap(),
        &PnlFilter {
            account_id: Some(account_id.to_string()),
            instrument_id: Some(instrument_id.to_string()),
        },
    )
    .unwrap()
    .total
    .iter()
    .map(|c| c.realized_pnl_cents)
    .sum()
}

/// 份额调整前后（持仓仍在）的四路读数。
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

/// 迁移侧真实数据的端到端验收（spec #1045 决策 12 / issue #1055）：
/// 6 笔结转实写 → 持仓补足 5,820.53 份 → 被阻塞的真实卖出走通。
#[tokio::test]
async fn test_split_real_migration_data_unblocks_blocked_sell_end_to_end() {
    // 502010 经场外基金通道按代码创建（东财桩离线驱动）：类型 fund、市场 unknown，
    // 创建带回的权威净值落 `market_prices` 现价——即持仓视图消费的价格通道。
    let hits = HashMap::from([(
        "502010".to_string(),
        FundStubHit {
            name: "易方达中证全指证券公司指数(LOF)A",
            fund_class: "指数型-股票",
            nav: Some((1.3281, "2026-06-26")),
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

    // 迁移侧结转前持仓 5,480.77 份经唯一写入面落账（基金金额权威）。
    let buy = format!(
        r#"{{"kind":"buy","amount_cents":{BUY_AMOUNT_CENTS},"currency_code":"CNY","account_id":"{account_id}","date":"2018-12-20","instrument_id":"{instrument_id}","quantity":{QUANTITY_BEFORE},"fee_cents":0}}"#
    );
    let created = post_batch(&app, batch_body(&[buy.as_str()], None)).await;
    assert_eq!(created[0]["success"], true, "建仓应成功: {created:?}");

    // 被阻塞的真实卖出：结转落账前 339.76 份缺口触发可卖出数量守卫——这正是迁移侧
    // `qieman-2026-06` 卡在重迁中间态的真实成因。
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

    // 份额调整前快照：账户余额清单（HTTP）+ 持仓视图 / 净资产 / 盈亏（域读）。
    let (_, balances_before) = get_json(&app, "/api/v1/accounts/balances").await;
    let before = readout(&conn, &account_id, &instrument_id);
    assert!(
        (before.holding.quantity - QUANTITY_BEFORE).abs() < 1e-6,
        "建仓后持仓应为 5,480.77，实际 {}",
        before.holding.quantity
    );

    // 6 笔真实结转实写落账（无金额 / 无手续费 / 无出资账户）。
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

    // 验收 1 锚点：持仓 5,480.77 + 339.76 = 5,820.53 份。
    let after = readout(&conn, &account_id, &instrument_id);
    assert!(
        (after.holding.quantity - QUANTITY_AFTER).abs() < 1e-6,
        "结转后持仓应为 5,820.53，实际 {}",
        after.holding.quantity
    );

    // 验收 2 锚点：账户余额清单（含全部可见账户）落账前后完全不变——无现金腿。
    let (_, balances_after) = get_json(&app, "/api/v1/accounts/balances").await;
    assert_eq!(
        balances_before, balances_after,
        "份额调整落账前后全部账户余额应完全不变"
    );

    // 验收 3 锚点：份额换手 = 持仓视图前后差 = +339.76 份，与 6 笔结转 Δ 各自独立对拍。
    let turnover = after.holding.quantity - before.holding.quantity;
    assert!(
        (turnover - declared_increment).abs() < 1e-6,
        "持仓视图前后差应等于 6 笔结转 Δ 合计 +{declared_increment}（份额换手），实际 {turnover}"
    );

    // 验收 4 锚点：净资产与未实现盈亏按份额增量变化，已实现盈亏不变。
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
    // 未实现盈亏增量显式钉住两组数字（ADR-0106 决策 7）：市值 727,901 → 773,025，
    // 成本展示值 548,077 → 548,061——每份成本整数化在 v_holdings 的亚分残差随数量放大
    // （权威总成本锚点口径精确不变，见域单测），故增量 45,140 = 市值增量 45,124 + 16。
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

    // 落库读回：6 条结转行以 kind=split 可见（筛选闭集本就含 split）。
    let (_, list) = get_json(&app, "/api/v1/transactions?kinds=split").await;
    let items = list["items"].as_array().expect("读回应为 {items, total}");
    assert_eq!(items.len(), 6, "6 笔结转应逐条可读回");
    assert!(
        items
            .iter()
            .all(|t| t["kind"] == "split" && t["amount_cents"] == 0),
        "split 行金额应恒为 0: {items:?}"
    );

    // 验收 1 收尾：被 339.76 份缺口阻塞的真实卖出实写走通；清仓后已实现盈亏按
    // 权威金额闭合（7,730.25 − 5,480.77 = 2,249.48 元）。
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
