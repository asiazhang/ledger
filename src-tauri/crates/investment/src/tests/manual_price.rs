use rusqlite::params;

use crate::manual_price::record_manual_price;
use crate::{InstrumentInput, InstrumentType, ManualPriceInput, ManualPriceResult};
use tauri_app_lib::test_support::{open, seed_instrument};

fn quote_input(instrument_id: &str, date: &str, price_cents: i64) -> ManualPriceInput {
    ManualPriceInput {
        instrument_id: instrument_id.to_string(),
        date: date.to_string(),
        price_cents,
    }
}

fn quote(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    date: &str,
    price_cents: i64,
) -> ManualPriceResult {
    record_manual_price(conn, &quote_input(instrument_id, date, price_cents)).expect("录价失败")
}

type PriceRow = (i64, String, String, Option<String>, Option<String>);

fn price_row(conn: &rusqlite::Connection, instrument_id: &str) -> Option<PriceRow> {
    conn.query_row(
        "SELECT price_cents, currency_code, priced_at, nav_date, source FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        },
    )
    .ok()
}

fn week_point(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    any_day_in_week: &str,
) -> Option<(String, i64, String, String)> {
    conn.query_row(
        "SELECT trade_date, price_cents, currency_code, source FROM price_history \
         WHERE instrument_id=?1 AND week_start = date(?2,'-6 days','weekday 1')",
        params![instrument_id, any_day_in_week],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .ok()
}

fn history_count(conn: &rusqlite::Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM price_history WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn price_written_evidence_pins_all_outcome_shapes() {
    use ledger_infra::signals::{Signal, WriteEvidence as E, WriteOp as Op, signals_for};

    fn assert_signals(actual: &[Signal], expected: &[Signal]) {
        assert_eq!(actual, expected);
    }
    let signals = |r: &ManualPriceResult| {
        signals_for(Op::RecordManualPrice, E::PriceWritten(r.any_written()))
    };

    assert_signals(
        signals(&ManualPriceResult {
            history_written: true,
            current_price_written: true,
        }),
        &[Signal::PricesChanged],
    );
    assert_signals(
        signals(&ManualPriceResult {
            history_written: true,
            current_price_written: false,
        }),
        &[Signal::PricesChanged],
    );
    assert_signals(
        signals(&ManualPriceResult {
            history_written: false,
            current_price_written: false,
        }),
        &[],
    );
}

#[test]
fn quote_writes_both_landings_with_manual_source() {
    let conn = open();
    seed_instrument(&conn, "inst-1", "inst-1", "测试标的", "CNY", "unknown");

    let outcome = quote(&conn, "inst-1", "2026-08-28", 13180);

    assert!(outcome.history_written);
    assert!(outcome.current_price_written);
    let price = price_row(&conn, "inst-1").expect("现价缓存应有行");
    assert_eq!(price.0, 13180);
    assert_eq!(price.1, "CNY");
    assert_eq!(price.2, "2026-08-28");
    assert_eq!(price.3, None, "手动落价无净值日期语义");
    assert_eq!(price.4.as_deref(), Some("manual"));
    let point = week_point(&conn, "inst-1", "2026-08-28").expect("价格历史应有周点");
    assert_eq!(point.0, "2026-08-28");
    assert_eq!(point.1, 13180);
    assert_eq!(point.2, "CNY");
    assert_eq!(point.3, "manual");
}

#[test]
fn quote_reuses_instrument_currency_and_is_idempotent_per_week() {
    let conn = open();
    seed_instrument(&conn, "inst-hkd", "inst-hkd", "测试标的", "HKD", "unknown");

    quote(&conn, "inst-hkd", "2026-08-24", 10000);
    quote(&conn, "inst-hkd", "2026-08-24", 10500);

    assert_eq!(history_count(&conn, "inst-hkd"), 1);
    let point = week_point(&conn, "inst-hkd", "2026-08-24").expect("周点应存在");
    assert_eq!(point.1, 10500);
    assert_eq!(price_row(&conn, "inst-hkd").unwrap().0, 10500);
}

#[test]
fn same_week_later_quote_overwrites_whole_week_and_moves_current_price() {
    let conn = open();
    seed_instrument(&conn, "inst-2", "inst-2", "测试标的", "CNY", "unknown");

    quote(&conn, "inst-2", "2026-08-24", 10000);
    let outcome = quote(&conn, "inst-2", "2026-08-27", 12000);

    assert!(outcome.history_written);
    assert!(
        outcome.current_price_written,
        "同周后写仍为最新点，现价随之更新"
    );
    assert_eq!(history_count(&conn, "inst-2"), 1, "同周至多一条周点");
    let point = week_point(&conn, "inst-2", "2026-08-24").expect("周点应存在");
    assert_eq!(point.0, "2026-08-27", "整周覆盖：trade_date 随后写");
    assert_eq!(point.1, 12000);
    let price = price_row(&conn, "inst-2").expect("现价缓存应有行");
    assert_eq!(price.0, 12000);
    assert_eq!(price.2, "2026-08-27");
}

#[test]
fn backfill_older_than_latest_settles_history_only() {
    let conn = open();
    seed_instrument(&conn, "inst-3", "inst-3", "测试标的", "CNY", "unknown");

    quote(&conn, "inst-3", "2026-08-28", 12000);
    let outcome = quote(&conn, "inst-3", "2026-08-05", 10000);

    assert!(outcome.history_written);
    assert!(!outcome.current_price_written, "回填旧价不改变现价");
    assert_eq!(history_count(&conn, "inst-3"), 2, "旧价沉淀为独立周点");
    let backfilled = week_point(&conn, "inst-3", "2026-08-05").expect("回填周点应存在");
    assert_eq!(backfilled.1, 10000);
    let price = price_row(&conn, "inst-3").expect("现价缓存应有行");
    assert_eq!(price.0, 12000);
    assert_eq!(price.2, "2026-08-28");
}

#[test]
fn backfill_same_week_overwrite_of_old_point_still_keeps_current_price() {
    let conn = open();
    seed_instrument(&conn, "inst-4", "inst-4", "测试标的", "CNY", "unknown");

    quote(&conn, "inst-4", "2026-08-28", 12000);
    quote(&conn, "inst-4", "2026-08-05", 10000);
    let outcome = quote(&conn, "inst-4", "2026-08-06", 11000);

    assert!(outcome.history_written);
    assert!(!outcome.current_price_written);
    assert_eq!(history_count(&conn, "inst-4"), 2, "同周覆盖不产生新行");
    let corrected = week_point(&conn, "inst-4", "2026-08-05").expect("旧周点应存在");
    assert_eq!(corrected.0, "2026-08-06", "整周覆盖：trade_date 随后写");
    assert_eq!(corrected.1, 11000);
    let price = price_row(&conn, "inst-4").expect("现价缓存应有行");
    assert_eq!(price.0, 12000, "回填与修正旧价均不动现价");
    assert_eq!(price.2, "2026-08-28");
}

#[test]
fn quote_newer_than_latest_moves_current_price_forward() {
    let conn = open();
    seed_instrument(&conn, "inst-5", "inst-5", "测试标的", "CNY", "unknown");

    quote(&conn, "inst-5", "2026-08-05", 10000);
    let outcome = quote(&conn, "inst-5", "2026-08-28", 13180);

    assert!(
        outcome.current_price_written,
        "新报价成为最新点，现价随之更新"
    );
    let price = price_row(&conn, "inst-5").expect("现价缓存应有行");
    assert_eq!(price.0, 13180);
    assert_eq!(price.2, "2026-08-28");
    assert_eq!(history_count(&conn, "inst-5"), 2);
}

#[test]
fn quote_rejects_non_positive_price() {
    let conn = open();
    seed_instrument(&conn, "inst-6", "inst-6", "测试标的", "CNY", "unknown");

    for bad in [0, -1] {
        let err = record_manual_price(&conn, &quote_input("inst-6", "2026-08-28", bad))
            .expect_err("非正价格应被拒绝");
        assert!(
            err.to_string().contains("价格必须大于 0"),
            "实际错误：{err}"
        );
    }
    assert_eq!(history_count(&conn, "inst-6"), 0, "校验失败不落库");
    assert!(price_row(&conn, "inst-6").is_none());
}

#[test]
fn quote_rejects_malformed_date() {
    let conn = open();
    seed_instrument(&conn, "inst-7", "inst-7", "测试标的", "CNY", "unknown");

    for bad in ["2026/08/28", "not-a-date", "", "2026-13-40"] {
        let err = record_manual_price(&conn, &quote_input("inst-7", bad, 10000))
            .expect_err("非法日期应被拒绝");
        assert!(err.to_string().contains("日期格式"), "实际错误：{err}");
    }
    assert_eq!(history_count(&conn, "inst-7"), 0, "校验失败不落库");
}

#[test]
fn quote_canonicalizes_lenient_date_input() {
    let conn = open();
    seed_instrument(&conn, "inst-7b", "inst-7b", "测试标的", "CNY", "unknown");

    let outcome = quote(&conn, "inst-7b", "2026-8-8", 10000);
    assert!(outcome.history_written);
    let point = week_point(&conn, "inst-7b", "2026-08-08").expect("周点应存在");
    assert_eq!(point.0, "2026-08-08");
}

#[test]
fn quote_rejects_unknown_instrument() {
    let conn = open();

    let err = record_manual_price(&conn, &quote_input("no-such", "2026-08-28", 10000))
        .expect_err("不存在的标的应被拒绝");
    assert!(err.to_string().contains("标的不存在"), "实际错误：{err}");
    assert_eq!(history_count(&conn, "no-such"), 0);
}

#[test]
fn quote_after_upsert_reuse_keeps_working_on_existing_instrument() {
    let conn = open();
    let id = crate::create_instrument(
        &conn,
        InstrumentInput {
            symbol: "稳稳地幸福".into(),
            kind: InstrumentType::Other,
            name: Some("稳稳地幸福".into()),
            currency_code: "CNY".into(),
            market: None,
        },
    )
    .expect("创建标的失败");

    let outcome = quote(&conn, &id, "2026-08-28", 13180);
    assert!(outcome.history_written && outcome.current_price_written);
    let price = price_row(&conn, &id).expect("现价缓存应有行");
    assert_eq!(price.0, 13180);
    assert_eq!(price.1, "CNY", "币种随标的字典（创建时币种可选）");
}
