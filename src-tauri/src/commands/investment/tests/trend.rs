use crate::commands::transactions::create_transaction_internal;
use crate::transaction::amount::TransactionKind;
use rusqlite::{Connection, params};

use super::super::*;
use super::common::*;

// ---------------------------------------------------------------------------
// 走势查询（issue #138 / spec #135 / ADR-0019）
// ---------------------------------------------------------------------------

/// 直插一条价格历史周点行（走势查询为只读命令，绕过采集通道直接铺样例数据）。
fn insert_price_history(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
    currency: &str,
) {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, instrument_id, trade_date, price_cents, currency],
    )
    .unwrap();
}

/// 直插一条汇率历史周点行（1 base = rate quote）。
fn insert_fx_rate_history(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    trade_date: &str,
    rate: f64,
) {
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, base, quote, trade_date, rate],
    )
    .unwrap();
}

#[test]
fn instrument_price_trend_clips_range_and_starts_at_first_point() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-t1", "600519", "贵州茅台", "CNY");
    insert_price_history(&conn, "ph-1", "inst-t1", "2026-01-05", 10000, "CNY");
    insert_price_history(&conn, "ph-2", "inst-t1", "2026-01-12", 11000, "CNY");
    insert_price_history(&conn, "ph-3", "inst-t1", "2026-01-19", 12000, "CNY");
    insert_price_history(&conn, "ph-4", "inst-t1", "2026-02-02", 13000, "CNY");

    // 区间裁剪：只返回区间内（含端点）的周点。
    let trend = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-01-10".into()),
            end_date: Some("2026-01-31".into()),
        },
    )
    .unwrap();
    let dates: Vec<&str> = trend.points.iter().map(|p| p.date.as_str()).collect();
    assert_eq!(dates, ["2026-01-12", "2026-01-19"]);
    assert_eq!(trend.points[0].price_cents, 11000);
    assert_eq!(trend.points[0].currency_code, "CNY");
    assert_eq!(trend.instrument_id, "inst-t1");

    // 不设界（"全部"区间）：从首个有效采样点开始，升序完整返回。
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-t1", &TrendRange::default()).unwrap();
    assert_eq!(trend.points.len(), 4);
    assert_eq!(trend.points[0].date, "2026-01-05");

    // 区间参数非法时报错，不静默返回曲线。
    let err = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-13-01".into()),
            end_date: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Invalid(_)));
    let err = trend::query_instrument_price_trend(
        &conn,
        "inst-t1",
        &TrendRange {
            start_date: Some("2026-02-01".into()),
            end_date: Some("2026-01-01".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Invalid(_)));
}

#[test]
fn portfolio_trend_derives_quantity_from_buy_sell_flow() {
    let conn = setup_db();
    insert_account(&conn, "acc-trd", "证券户", "investment", "CNY");
    insert_instrument(&conn, "inst-t2", "000001", "平安银行", "CNY");
    // 周价格点：w1=1000、w2=2000、w3=3000、w4=4000（CNY，无需折算）。
    insert_price_history(&conn, "ph-w1", "inst-t2", "2026-02-02", 1000, "CNY");
    insert_price_history(&conn, "ph-w2", "inst-t2", "2026-02-09", 2000, "CNY");
    insert_price_history(&conn, "ph-w3", "inst-t2", "2026-02-16", 3000, "CNY");
    insert_price_history(&conn, "ph-w4", "inst-t2", "2026-02-23", 4000, "CNY");
    // 时序：w1 未买入（数量 0）→ w2 周内（02-06）买入 10 股 → w3 持有 10 股 → 2026-02-20（w3 内）清仓 → w4 归零。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-trd",
            "inst-t2",
            10.0,
            1500,
            "2026-02-06",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-trd",
            "inst-t2",
            10.0,
            3500,
            "2026-02-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-02-02".to_string(), 0),     // 买入前：价格有效但持有为零
            ("2026-02-09".to_string(), 20000), // 10 × 2000
            ("2026-02-16".to_string(), 30000), // 10 × 3000（卖出在 02-20，尚未生效）
            ("2026-02-23".to_string(), 0),     // 清仓后归零
        ]
    );
}

#[test]
fn portfolio_trend_with_date_range_clips_weeks_and_does_not_lose_pre_start_flow() {
    let conn = setup_db();
    insert_account(&conn, "acc-rng", "区间户", "investment", "CNY");
    insert_instrument(&conn, "inst-rng", "600036", "招商银行", "CNY");
    insert_price_history(&conn, "ph-r1", "inst-rng", "2026-04-06", 1000, "CNY");
    insert_price_history(&conn, "ph-r2", "inst-rng", "2026-04-13", 2000, "CNY");
    insert_price_history(&conn, "ph-r3", "inst-rng", "2026-04-20", 4000, "CNY");
    // 买入在区间起点之前：起点前的流水必须累积带入，起点后各周数量才非零。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-rng",
            "inst-rng",
            3.0,
            500,
            "2026-04-08",
        ),
    )
    .unwrap();

    // 回归（#138 评审）：带 start_date 的组合走势查询曾因流水查询占位符
    // 与参数个数不匹配而运行时报错；此处同时锁定区间裁剪与起点前持仓带入。
    let trend = trend::query_portfolio_value_trend(
        &conn,
        &TrendRange {
            start_date: Some("2026-04-10".into()),
            end_date: Some("2026-04-21".into()),
        },
    )
    .unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-04-13".to_string(), 6000),  // 3 × 2000（起点前买入已带入）
            ("2026-04-20".to_string(), 12000), // 3 × 4000
        ]
    );
}

#[test]
fn portfolio_trend_converts_hkd_via_same_week_fx_with_reverse_fallback() {
    let conn = setup_db();
    insert_account(&conn, "acc-hkd", "港美股户", "investment", "CNY");
    insert_instrument(&conn, "inst-hkd", "00700", "腾讯控股", "HKD");
    // 港股以 HKD 计价：w1=100 HKD（10000 分）、w2=200 HKD、w3=300 HKD。
    insert_price_history(&conn, "ph-h1", "inst-hkd", "2026-03-02", 10000, "HKD");
    insert_price_history(&conn, "ph-h2", "inst-hkd", "2026-03-09", 20000, "HKD");
    insert_price_history(&conn, "ph-h3", "inst-hkd", "2026-03-16", 30000, "HKD");
    // w1 有正向汇率 HKD->CNY=0.8；w2 只有反向 CNY->HKD=5.0（兜底取倒数 0.2）；w3 无任何历史汇率。
    insert_fx_rate_history(&conn, "fx-h1", "HKD", "CNY", "2026-03-03", 0.8);
    insert_fx_rate_history(&conn, "fx-h2", "CNY", "HKD", "2026-03-10", 5.0);
    // 2 股，全程持有（买入早于首条价格点）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-hkd",
            "inst-hkd",
            2.0,
            10000,
            "2026-02-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    // w1: 2×10000×0.8=16000；w2: 2×20000×(1/5.0)=8000；w3 缺同期汇率 → 该周被跳过（不伪造数据）。
    assert_eq!(
        values,
        [
            ("2026-03-02".to_string(), 16000),
            ("2026-03-09".to_string(), 8000),
        ]
    );
}

#[test]
fn portfolio_trend_skips_weeks_missing_price_or_fx_but_keeps_other_contributors() {
    let conn = setup_db();
    insert_account(&conn, "acc-mix", "混合户", "investment", "CNY");
    insert_instrument(&conn, "inst-a", "600000", "浦发银行", "CNY");
    insert_instrument(&conn, "inst-b", "09988", "阿里巴巴", "HKD");
    // 各买 1 股，早于首条价格点。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mix",
            "inst-a",
            1.0,
            1000,
            "2026-02-20",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mix",
            "inst-b",
            1.0,
            10000,
            "2026-02-20",
        ),
    )
    .unwrap();
    // inst-a（CNY）三周全有价：1000 分/周。
    insert_price_history(&conn, "ph-a1", "inst-a", "2026-03-02", 1000, "CNY");
    insert_price_history(&conn, "ph-a2", "inst-a", "2026-03-09", 1000, "CNY");
    insert_price_history(&conn, "ph-a3", "inst-a", "2026-03-16", 1000, "CNY");
    // inst-b（HKD）w2 整周无价（停牌语义）；w3 有价但缺同期汇率。
    insert_price_history(&conn, "ph-b1", "inst-b", "2026-03-02", 10000, "HKD");
    insert_price_history(&conn, "ph-b3", "inst-b", "2026-03-16", 10000, "HKD");
    // 仅 w1 有 HKD->CNY=0.9。
    insert_fx_rate_history(&conn, "fx-m1", "HKD", "CNY", "2026-03-03", 0.9);

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    // w1: 1000 + 10000×0.9=10000；w2: inst-b 缺价被跳过，仅 inst-a 1000；w3: inst-b 缺汇率被跳过，仅 inst-a 1000。
    assert_eq!(
        values,
        [
            ("2026-03-02".to_string(), 10000),
            ("2026-03-09".to_string(), 1000),
            ("2026-03-16".to_string(), 1000),
        ]
    );
}

#[test]
fn trend_commands_return_empty_state_without_history() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-empty", "000002", "万科A", "CNY");

    // 无任何价格历史：单标的与组合走势都返回空态结构（points 为空）。
    let trend =
        trend::query_instrument_price_trend(&conn, "inst-empty", &TrendRange::default()).unwrap();
    assert_eq!(trend.instrument_id, "inst-empty");
    assert!(trend.points.is_empty());

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    assert!(trend.points.is_empty());
}

#[test]
fn portfolio_trend_excludes_soft_deleted_account_flow_including_history() {
    // 软删除账户口径（issue #247 / #217 定案 Q1「账户已删」）：组合走势逐期
    // 数量推算经时点持仓接缝排除软删账户的 buy/sell 流水——「今天」与历史
    // 周采样点全部剔除，与 v_holdings（空）对齐；删除/恢复经软删标志翻转
    // 自动进出推算，无时点存续状态。
    let conn = setup_db();
    insert_account(&conn, "acc-sd", "待删户", "investment", "CNY");
    insert_instrument(&conn, "inst-sd", "000001", "平安银行", "CNY");
    // 周价格点：w1=1000、w2=2000、w3=3000（CNY，无需折算）。
    insert_price_history(&conn, "ph-s1", "inst-sd", "2026-02-02", 1000, "CNY");
    insert_price_history(&conn, "ph-s2", "inst-sd", "2026-02-09", 2000, "CNY");
    insert_price_history(&conn, "ph-s3", "inst-sd", "2026-02-16", 3000, "CNY");
    // 早于首条价格点买入 10 股：软删前基线 = 各周 10 × 当周价。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sd",
            "inst-sd",
            10.0,
            1500,
            "2026-01-20",
        ),
    )
    .unwrap();

    let values = |conn: &Connection| -> Vec<(String, i64)> {
        trend::query_portfolio_value_trend(conn, &TrendRange::default())
            .unwrap()
            .points
            .iter()
            .map(|p| (p.date.clone(), p.market_value_cents))
            .collect()
    };
    let baseline = values(&conn);
    assert_eq!(
        baseline,
        [
            ("2026-02-02".to_string(), 10000),
            ("2026-02-09".to_string(), 20000),
            ("2026-02-16".to_string(), 30000),
        ]
    );

    // 软删除账户（只翻 is_deleted 标志）：历史周采样点同样不再含该账户贡献。
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-sd'", [])
        .unwrap();
    assert_eq!(
        values(&conn),
        [
            ("2026-02-02".to_string(), 0),
            ("2026-02-09".to_string(), 0),
            ("2026-02-16".to_string(), 0),
        ]
    );

    // 绑定不变式（spec #168 定案第 6 条 / #217 Q4）：含软删夹具下
    // as-of「今天」≡ Holding——Holding 侧 v_holdings 无行，as-of 侧同日推算为 0。
    let holding_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-sd'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holding_rows, 0, "Holding 侧：软删账户批次不入 v_holdings");
    let qty = holdings::holdings_as_of(&conn, Some("inst-sd"), "2026-06-01").unwrap();
    assert!((qty - 0.0).abs() < 1e-9, "as-of「今天」= {qty}");

    // 恢复账户（标志翻回）：流水自动回到推算，走势与基线逐点一致——口径可逆。
    conn.execute("UPDATE accounts SET is_deleted=0 WHERE id='acc-sd'", [])
        .unwrap();
    assert_eq!(values(&conn), baseline);
}

#[test]
fn portfolio_trend_keeps_hidden_account_flow() {
    // 隐藏 ≠ 删除（#217 定案 Q2）：隐藏账户（is_hidden）不是软删除，其 buy/sell
    // 流水照常计入逐期推算，与 v_holdings 不排隐藏账户一致。
    let conn = setup_db();
    insert_account(&conn, "acc-hid", "隐藏户", "investment", "CNY");
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hid'", [])
        .unwrap();
    insert_instrument(&conn, "inst-hd", "600036", "招商银行", "CNY");
    insert_price_history(&conn, "ph-hd1", "inst-hd", "2026-02-02", 1000, "CNY");
    insert_price_history(&conn, "ph-hd2", "inst-hd", "2026-02-09", 2000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-hid",
            "inst-hd",
            10.0,
            1500,
            "2026-01-20",
        ),
    )
    .unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-02-02".to_string(), 10000),
            ("2026-02-09".to_string(), 20000),
        ]
    );
    // Holding 侧同口径：v_holdings 不排隐藏账户，持仓行仍在。
    let holding_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-hd'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holding_rows, 1);
}
