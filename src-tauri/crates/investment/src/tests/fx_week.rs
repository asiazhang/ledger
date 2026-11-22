use std::collections::HashMap;

use chrono::NaiveDate;

use crate::fx_week::nearest_week_fx_rate;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("ISO 日期")
}

fn fx(rows: &[(&str, &str, &str, f64)]) -> HashMap<(String, String), HashMap<String, f64>> {
    let mut map: HashMap<(String, String), HashMap<String, f64>> = HashMap::new();
    for (base, quote, week, rate) in rows {
        map.entry((base.to_string(), quote.to_string()))
            .or_default()
            .insert(week.to_string(), *rate);
    }
    map
}

#[test]
fn exact_week_hit_uses_event_week_rate() {
    let history = fx(&[("USD", "CNY", "2026-01-05", 7.2)]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-07")),
        Some(7.2)
    );
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-11")),
        Some(7.2)
    );
}

#[test]
fn nearest_earlier_week_wins_within_window() {
    let history = fx(&[
        ("USD", "CNY", "2025-12-29", 7.1),
        ("USD", "CNY", "2026-01-26", 7.4),
    ]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-06")),
        Some(7.1)
    );
}

#[test]
fn nearest_later_week_wins_when_no_earlier_point() {
    let history = fx(&[("USD", "CNY", "2026-01-12", 7.3)]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-08")),
        Some(7.3)
    );
}

#[test]
fn window_edge_week_eight_hits() {
    let earlier = fx(&[("USD", "CNY", "2025-11-10", 7.05)]);
    assert_eq!(
        nearest_week_fx_rate(&earlier, "USD", "CNY", date("2026-01-05")),
        Some(7.05)
    );
    let later = fx(&[("USD", "CNY", "2026-03-02", 7.15)]);
    assert_eq!(
        nearest_week_fx_rate(&later, "USD", "CNY", date("2026-01-05")),
        Some(7.15)
    );
}

#[test]
fn week_nine_is_out_of_window_missing() {
    let earlier = fx(&[("USD", "CNY", "2025-11-03", 7.05)]);
    assert_eq!(
        nearest_week_fx_rate(&earlier, "USD", "CNY", date("2026-01-05")),
        None
    );
    let later = fx(&[("USD", "CNY", "2026-03-09", 7.15)]);
    assert_eq!(
        nearest_week_fx_rate(&later, "USD", "CNY", date("2026-01-05")),
        None
    );
}

#[test]
fn empty_window_returns_missing_signal() {
    assert_eq!(
        nearest_week_fx_rate(&HashMap::new(), "USD", "CNY", date("2026-01-05")),
        None
    );
}

#[test]
fn equidistant_gap_prefers_earlier_week() {
    let history = fx(&[
        ("USD", "CNY", "2025-12-29", 7.1),
        ("USD", "CNY", "2026-01-12", 7.3),
    ]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-05")),
        Some(7.1)
    );
}

#[test]
fn same_currency_identity_without_history() {
    assert_eq!(
        nearest_week_fx_rate(&HashMap::new(), "CNY", "CNY", date("2026-01-05")),
        Some(1.0)
    );
}

#[test]
fn reverse_pair_backfills_at_event_week() {
    let history = fx(&[("CNY", "USD", "2026-01-05", 0.25)]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-07")),
        Some(4.0)
    );
}

#[test]
fn reverse_pair_backfills_at_nearest_week() {
    let history = fx(&[("CNY", "USD", "2025-12-29", 0.5)]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-05")),
        Some(2.0)
    );
}
