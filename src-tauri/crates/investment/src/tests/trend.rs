use ledger_transaction::amount::TransactionKind;
use ledger_transaction::create_transaction_internal;
use rusqlite::Connection;
use tauri_app_lib::test_support::{
    open, seed_account, seed_fx_rate_history, seed_instrument, seed_price_history,
};

use super::super::*;
use super::common::*;

#[test]
fn portfolio_trend_validates_range_format_and_order() {
    let conn = open();
    seed_instrument(&conn, "inst-t1", "600519", "贵州茅台", "CNY", "unknown");

    let err = trend::query_portfolio_value_trend(
        &conn,
        &TrendRange {
            start_date: Some("2026-13-01".into()),
            end_date: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    let err = trend::query_portfolio_value_trend(
        &conn,
        &TrendRange {
            start_date: Some("2026-02-01".into()),
            end_date: Some("2026-01-01".into()),
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
}
#[test]
fn portfolio_trend_derives_quantity_from_buy_sell_flow() {
    let conn = open();
    seed_account(&conn, "acc-trd", "证券户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-t2", "000001", "平安银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-w1", "inst-t2", "2026-02-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-w2", "inst-t2", "2026-02-09", 200_000, "CNY");
    seed_price_history(&conn, "ph-w3", "inst-t2", "2026-02-16", 300_000, "CNY");
    seed_price_history(&conn, "ph-w4", "inst-t2", "2026-02-23", 400_000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-trd",
            "inst-t2",
            10.0,
            150_000,
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
            350_000,
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
            ("2026-02-02".to_string(), 0),
            ("2026-02-09".to_string(), 20000),
            ("2026-02-16".to_string(), 30000),
            ("2026-02-23".to_string(), 0),
        ]
    );
}

#[test]
fn portfolio_trend_with_date_range_clips_weeks_and_does_not_lose_pre_start_flow() {
    let conn = open();
    seed_account(&conn, "acc-rng", "区间户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-rng", "600036", "招商银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-r1", "inst-rng", "2026-04-06", 100_000, "CNY");
    seed_price_history(&conn, "ph-r2", "inst-rng", "2026-04-13", 200_000, "CNY");
    seed_price_history(&conn, "ph-r3", "inst-rng", "2026-04-20", 400_000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-rng",
            "inst-rng",
            3.0,
            50_000,
            "2026-04-08",
        ),
    )
    .unwrap();

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
            ("2026-04-13".to_string(), 6000),
            ("2026-04-20".to_string(), 12000),
        ]
    );
}

#[test]
fn portfolio_trend_converts_hkd_via_same_week_fx_with_reverse_fallback() {
    let conn = open();
    seed_account(&conn, "acc-hkd", "港美股户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-hkd", "00700", "腾讯控股", "HKD", "unknown");
    seed_price_history(&conn, "ph-h1", "inst-hkd", "2026-03-02", 1_000_000, "HKD");
    seed_price_history(&conn, "ph-h2", "inst-hkd", "2026-03-09", 2_000_000, "HKD");
    seed_price_history(&conn, "ph-h3", "inst-hkd", "2026-03-16", 3_000_000, "HKD");
    seed_fx_rate_history(&conn, "fx-h1", "HKD", "CNY", "2026-03-03", 0.8);
    seed_fx_rate_history(&conn, "fx-h2", "CNY", "HKD", "2026-03-10", 5.0);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-hkd",
            "inst-hkd",
            2.0,
            1_000_000,
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
            ("2026-03-02".to_string(), 16000),
            ("2026-03-09".to_string(), 8000),
            ("2026-03-16".to_string(), 12000),
        ]
    );
}

#[test]
fn portfolio_trend_carries_previous_week_price_and_fx_on_gaps() {
    let conn = open();
    seed_account(&conn, "acc-mix", "混合户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-a", "600000", "浦发银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-b", "09988", "阿里巴巴", "HKD", "unknown");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-mix",
            "inst-a",
            1.0,
            100_000,
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
            1_000_000,
            "2026-02-20",
        ),
    )
    .unwrap();
    seed_price_history(&conn, "ph-a1", "inst-a", "2026-03-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-a2", "inst-a", "2026-03-09", 100_000, "CNY");
    seed_price_history(&conn, "ph-a3", "inst-a", "2026-03-16", 100_000, "CNY");
    seed_price_history(&conn, "ph-b1", "inst-b", "2026-03-02", 1_000_000, "HKD");
    seed_price_history(&conn, "ph-b3", "inst-b", "2026-03-16", 1_000_000, "HKD");
    seed_fx_rate_history(&conn, "fx-m1", "HKD", "CNY", "2026-03-03", 0.9);

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2026-03-02".to_string(), 10000),
            ("2026-03-09".to_string(), 10000),
            ("2026-03-16".to_string(), 10000),
        ]
    );
}

#[test]
fn portfolio_trend_returns_empty_state_without_history() {
    let conn = open();
    seed_instrument(&conn, "inst-empty", "000002", "万科A", "CNY", "unknown");

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.currency_code, "CNY");
    assert!(trend.points.is_empty());
}
#[test]
fn portfolio_trend_excludes_soft_deleted_account_flow_including_history() {
    let conn = open();
    seed_account(&conn, "acc-sd", "待删户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-sd", "000001", "平安银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-s1", "inst-sd", "2026-02-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-s2", "inst-sd", "2026-02-09", 200_000, "CNY");
    seed_price_history(&conn, "ph-s3", "inst-sd", "2026-02-16", 300_000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sd",
            "inst-sd",
            10.0,
            150_000,
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

    conn.execute("UPDATE accounts SET is_deleted=0 WHERE id='acc-sd'", [])
        .unwrap();
    assert_eq!(values(&conn), baseline);
}

#[test]
fn portfolio_trend_keeps_hidden_account_flow() {
    let conn = open();
    seed_account(&conn, "acc-hid", "隐藏户", "investment", "CNY", 0);
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hid'", [])
        .unwrap();
    seed_instrument(&conn, "inst-hd", "600036", "招商银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-hd1", "inst-hd", "2026-02-02", 100_000, "CNY");
    seed_price_history(&conn, "ph-hd2", "inst-hd", "2026-02-09", 200_000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-hid",
            "inst-hd",
            10.0,
            150_000,
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
    let holding_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-hd'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(holding_rows, 1);
}

fn backfill_state_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

fn backfill_status(
    state: TrendBackfillState,
    done: Option<usize>,
    total: Option<usize>,
) -> TrendBackfillStatus {
    TrendBackfillStatus { state, done, total }
}

#[test]
fn portfolio_trend_backfill_aggregates_pending_instruments() {
    let _guard = backfill_state_lock();
    let conn = open();
    insert_fund_instrument(&conn, "inst-pf-a", "110022", "易方达消费行业");
    insert_fund_instrument(&conn, "inst-pf-b", "000001", "中国蓝图混合");

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert!(trend.points.is_empty());
    assert_eq!(
        trend.backfill,
        Some(backfill_status(TrendBackfillState::Running, None, None))
    );

    backfill::round_started(2);
    backfill::round_progress(1);
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::Running,
            Some(1),
            Some(2)
        ))
    );
    backfill::round_finished();

    backfill::record_attempt("inst-pf-a", backfill::BackfillAttempt::NoData);
    backfill::record_attempt("inst-pf-b", backfill::BackfillAttempt::Failed);
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(
            TrendBackfillState::RetryPending,
            None,
            None
        ))
    );

    backfill::record_attempt("inst-pf-b", backfill::BackfillAttempt::NoData);
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(
        trend.backfill,
        Some(backfill_status(TrendBackfillState::NoData, None, None))
    );

    seed_price_history(&conn, "ph-pf-a", "inst-pf-a", "2026-01-05", 10_000, "CNY");
    seed_price_history(&conn, "ph-pf-b", "inst-pf-b", "2026-01-05", 10_000, "CNY");
    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.backfill, None, "没有待补全标的不携带聚合状态");
}

fn mark_constant(conn: &Connection, instrument_id: &str) {
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = ?1",
        [instrument_id],
    )
    .unwrap();
}

fn fixed_today() -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()
}

#[test]
fn portfolio_trend_includes_constant_fund_contribution_without_price_rows() {
    let conn = open();
    seed_account(&conn, "acc-const", "货基户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    mark_constant(&conn, "inst-const");
    let mut buy = make_trade_input(
        TransactionKind::Buy,
        "acc-const",
        "inst-const",
        1000.0,
        10_000,
        "2026-02-05",
    );
    buy.amount_cents = 100_000;
    buy.price_cents = None;
    create_transaction_internal(&conn, buy).unwrap();

    let trend = trend::query_portfolio_value_trend_on(&conn, &TrendRange::default(), fixed_today())
        .unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2025-12-29".to_string(), 0),
            ("2026-01-05".to_string(), 0),
            ("2026-01-12".to_string(), 0),
            ("2026-01-19".to_string(), 0),
            ("2026-01-26".to_string(), 0),
            ("2026-02-02".to_string(), 100_000),
            ("2026-02-09".to_string(), 100_000),
            ("2026-02-16".to_string(), 100_000),
            ("2026-02-23".to_string(), 100_000),
            ("2026-03-02".to_string(), 100_000),
            ("2026-03-09".to_string(), 100_000),
        ]
    );
}

#[test]
fn portfolio_trend_constant_fund_ignores_flat_history_rows() {
    let conn = open();
    seed_account(&conn, "acc-const", "货基户", "investment", "CNY", 0);
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    mark_constant(&conn, "inst-const");
    let mut buy = make_trade_input(
        TransactionKind::Buy,
        "acc-const",
        "inst-const",
        1000.0,
        10_000,
        "2026-02-05",
    );
    buy.amount_cents = 100_000;
    buy.price_cents = None;
    create_transaction_internal(&conn, buy).unwrap();
    seed_price_history(&conn, "ph-flat", "inst-const", "2026-02-16", 20_000, "CNY");

    let trend = trend::query_portfolio_value_trend_on(&conn, &TrendRange::default(), fixed_today())
        .unwrap();
    let values: Vec<(String, i64)> = trend
        .points
        .iter()
        .map(|p| (p.date.clone(), p.market_value_cents))
        .collect();
    assert_eq!(
        values,
        [
            ("2025-12-29".to_string(), 0),
            ("2026-01-05".to_string(), 0),
            ("2026-01-12".to_string(), 0),
            ("2026-01-19".to_string(), 0),
            ("2026-01-26".to_string(), 0),
            ("2026-02-02".to_string(), 100_000),
            ("2026-02-09".to_string(), 100_000),
            ("2026-02-16".to_string(), 100_000,),
            ("2026-02-23".to_string(), 100_000),
            ("2026-03-02".to_string(), 100_000),
            ("2026-03-09".to_string(), 100_000),
        ],
        "平坦行（20000）既不被消费也不叠加，恒定标的按常量 1.0000 出数"
    );
}

fn reference_trend_points(
    conn: &Connection,
    price_rows: &[(&str, &str, i64, &str)],
    rate: impl Fn(&str, &str) -> Option<f64>,
) -> Vec<(String, i64)> {
    use std::collections::{BTreeMap, BTreeSet};
    let weeks: BTreeSet<&str> = price_rows.iter().map(|row| row.1).collect();
    let instruments: BTreeSet<&str> = price_rows.iter().map(|row| row.0).collect();
    let mut by_week: BTreeMap<String, (i64, bool)> = BTreeMap::new();
    for instrument_id in instruments {
        let mut carried: Option<(i64, &str)> = None;
        for &week in &weeks {
            if let Some(&(_, _, price, currency)) = price_rows
                .iter()
                .find(|row| row.0 == instrument_id && row.1 == week)
            {
                carried = Some((price, currency));
            }
            let Some((price, currency)) = carried else {
                continue;
            };
            let Some(rate) = weeks
                .range(..=week)
                .filter_map(|w| rate(currency, w))
                .last()
            else {
                continue;
            };
            let quantity = holdings::holdings_as_of(conn, Some(instrument_id), week).unwrap();
            let value = (quantity * price as f64 / prices::PRICE_UNITS_PER_FEN).round() as i64;
            let entry = by_week.entry(week.to_string()).or_insert((0, false));
            entry.0 += (value as f64 * rate).round() as i64;
            entry.1 = true;
        }
    }
    by_week
        .into_iter()
        .filter(|(_, (_, contributed))| *contributed)
        .map(|(date, (total, _))| (date, total))
        .collect()
}

#[test]
fn portfolio_trend_incremental_quantity_matches_per_row_as_of_reference() {
    let conn = open();
    seed_account(&conn, "acc-eq", "证券户", "investment", "CNY", 0);
    seed_account(&conn, "acc-eq-gone", "待删户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-eq-cny", "000001", "平安银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-eq-hkd", "00700", "腾讯控股", "HKD", "unknown");
    seed_instrument(
        &conn,
        "inst-eq-out",
        "000001.OF",
        "转出基金",
        "CNY",
        "unknown",
    );
    seed_instrument(
        &conn,
        "inst-eq-in",
        "000002.OF",
        "转入基金",
        "CNY",
        "unknown",
    );
    seed_instrument(&conn, "inst-eq-spl", "600036", "招商银行", "CNY", "unknown");
    seed_instrument(&conn, "inst-eq-gone", "000005", "华新城", "CNY", "unknown");

    for (kind, qty, price, date) in [
        (TransactionKind::Buy, 10.0, 150_000, "2026-02-04"),
        (TransactionKind::Buy, 5.0, 180_000, "2026-02-10"),
        (TransactionKind::Sell, 4.0, 260_000, "2026-02-18"),
    ] {
        create_transaction_internal(
            &conn,
            make_trade_input(kind, "acc-eq", "inst-eq-cny", qty, price, date),
        )
        .unwrap();
    }
    let mut deleted_buy = make_trade_input(
        TransactionKind::Buy,
        "acc-eq",
        "inst-eq-cny",
        2.0,
        180_000,
        "2026-02-11",
    );
    deleted_buy.note = Some("eq-to-delete".into());
    create_transaction_internal(&conn, deleted_buy).unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-eq",
            "inst-eq-hkd",
            2.0,
            1_000_000,
            "2026-02-05",
        ),
    )
    .unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-eq",
            "inst-eq-out",
            6.0,
            100_000,
            "2026-02-03",
        ),
    )
    .unwrap();
    let mut convert = make_convert_input(
        "acc-eq",
        "inst-eq-out",
        "inst-eq-in",
        6.0,
        12.0,
        10_000,
        10_000,
        0,
    );
    convert.date = "2026-02-12".into();
    create_transaction_internal(&conn, convert).unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-eq",
            "inst-eq-spl",
            10.0,
            100_000,
            "2026-02-06",
        ),
    )
    .unwrap();
    let mut split = make_split_input("acc-eq", "inst-eq-spl", 10.0);
    split.date = "2026-02-17".into();
    create_transaction_internal(&conn, split).unwrap();

    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-eq-gone",
            "inst-eq-gone",
            3.0,
            100_000,
            "2026-02-06",
        ),
    )
    .unwrap();

    let price_rows = [
        ("inst-eq-cny", "2026-02-09", 100_000, "CNY"),
        ("inst-eq-cny", "2026-02-16", 200_000, "CNY"),
        ("inst-eq-cny", "2026-02-23", 300_000, "CNY"),
        ("inst-eq-out", "2026-02-09", 100_000, "CNY"),
        ("inst-eq-out", "2026-02-16", 100_000, "CNY"),
        ("inst-eq-in", "2026-02-09", 50_000, "CNY"),
        ("inst-eq-in", "2026-02-16", 50_000, "CNY"),
        ("inst-eq-spl", "2026-02-09", 100_000, "CNY"),
        ("inst-eq-spl", "2026-02-16", 100_000, "CNY"),
        ("inst-eq-spl", "2026-02-23", 100_000, "CNY"),
        ("inst-eq-gone", "2026-02-09", 100_000, "CNY"),
        ("inst-eq-gone", "2026-02-16", 100_000, "CNY"),
        ("inst-eq-hkd", "2026-03-02", 1_000_000, "HKD"),
        ("inst-eq-hkd", "2026-03-09", 1_000_000, "HKD"),
        ("inst-eq-hkd", "2026-03-16", 1_000_000, "HKD"),
    ];
    for (iid, date, price, currency) in price_rows {
        seed_price_history(
            &conn,
            &format!("ph-eq-{iid}-{date}"),
            iid,
            date,
            price,
            currency,
        );
    }
    seed_fx_rate_history(&conn, "fx-eq-1", "HKD", "CNY", "2026-03-02", 0.9);
    seed_fx_rate_history(&conn, "fx-eq-2", "CNY", "HKD", "2026-03-09", 5.0);
    let rate = |currency: &str, week: &str| -> Option<f64> {
        match (currency, week) {
            ("CNY", _) => Some(1.0),
            ("HKD", "2026-03-02") => Some(0.9),
            ("HKD", "2026-03-09") => Some(1.0 / 5.0),
            _ => None,
        }
    };

    let trend_values = |conn: &Connection| -> Vec<(String, i64)> {
        trend::query_portfolio_value_trend(conn, &TrendRange::default())
            .unwrap()
            .points
            .iter()
            .map(|p| (p.date.clone(), p.market_value_cents))
            .collect()
    };

    let baseline = trend_values(&conn);
    assert_eq!(
        baseline,
        reference_trend_points(&conn, &price_rows, rate),
        "新路径与逐行 as-of 参照逐点全等"
    );
    assert_eq!(
        baseline,
        [
            ("2026-02-09".to_string(), 29_000),
            ("2026-02-16".to_string(), 53_000),
            ("2026-02-23".to_string(), 68_000),
            ("2026-03-02".to_string(), 86_000),
            ("2026-03-09".to_string(), 72_000),
            ("2026-03-16".to_string(), 72_000),
        ],
    );

    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE note='eq-to-delete'",
        [],
    )
    .unwrap();
    assert_eq!(
        trend_values(&conn),
        reference_trend_points(&conn, &price_rows, rate),
        "软删交易后仍逐点全等"
    );

    conn.execute(
        "UPDATE accounts SET is_deleted=1 WHERE id='acc-eq-gone'",
        [],
    )
    .unwrap();
    let after = trend_values(&conn);
    assert_eq!(
        after,
        reference_trend_points(&conn, &price_rows, rate),
        "软删账户后仍逐点全等"
    );
    assert_eq!(
        after,
        [
            ("2026-02-09".to_string(), 26_000),
            ("2026-02-16".to_string(), 46_000),
            ("2026-02-23".to_string(), 59_000),
            ("2026-03-02".to_string(), 77_000),
            ("2026-03-09".to_string(), 63_000),
            ("2026-03-16".to_string(), 63_000),
        ],
    );
}
