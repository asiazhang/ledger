use chrono::NaiveDate;

use crate::fund_nav::nav_window;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn nav_window_first_sync_backfills_two_years() {
    let (start, end) = nav_window(None, date("2026-08-29"));
    assert_eq!(start, "2024-08-29");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_incremental_starts_day_after_watermark() {
    let (start, end) = nav_window(Some("2026-01-30"), date("2026-08-29"));
    assert_eq!(start, "2026-01-31");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_illegal_watermark_falls_back_to_first_sync() {
    let (start, end) = nav_window(Some("not-a-date"), date("2026-08-29"));
    assert_eq!(start, "2024-08-29");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_boundary_watermark_near_window_start() {
    let (start, _) = nav_window(Some("2020-01-01"), date("2026-08-29"));
    assert_eq!(start, "2020-01-02");
}
