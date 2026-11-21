use chrono::NaiveDate;

use crate::as_of::AsOfValues;
use tauri_app_lib::test_support::{open, seed_instrument, seed_price_history};

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("ISO 日期")
}

#[test]
fn as_of_load_takes_latest_row_at_or_before_cutoff() {
    let conn = open();
    seed_instrument(&conn, "inst-asof", "000001", "平安银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-a1", "inst-asof", "2026-01-05", 1000, "CNY");
    seed_price_history(&conn, "ph-a2", "inst-asof", "2026-01-12", 2000, "CNY");
    seed_price_history(&conn, "ph-a3", "inst-asof", "2026-01-19", 3000, "CNY");

    let values = AsOfValues::load(&conn, date("2026-01-15")).unwrap();
    assert_eq!(values.market_value(10.0, "inst-asof", "CNY"), Some(200));

    let values = AsOfValues::load(&conn, date("2026-01-19")).unwrap();
    assert_eq!(values.market_value(10.0, "inst-asof", "CNY"), Some(300));
}

#[test]
fn as_of_load_is_missing_when_all_rows_fall_after_cutoff() {
    let conn = open();
    seed_instrument(
        &conn,
        "inst-asof-late",
        "600519",
        "贵州茅台",
        "CNY",
        "unknown",
    );
    seed_price_history(&conn, "ph-l1", "inst-asof-late", "2026-01-19", 3000, "CNY");

    let values = AsOfValues::load(&conn, date("2026-01-15")).unwrap();
    assert_eq!(values.market_value(10.0, "inst-asof-late", "CNY"), None);
}
