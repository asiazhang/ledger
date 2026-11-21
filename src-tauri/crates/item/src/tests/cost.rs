use chrono::NaiveDate;

use crate::cost::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn same_day_is_one_day() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 1), None).unwrap();
    assert_eq!(r.days, 1);
    assert_eq!(r.numerator_cents, 10_000);
    assert_eq!(r.per_day_cents, 10_000.0);
}

#[test]
fn days_are_inclusive_of_both_ends() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 2), None).unwrap();
    assert_eq!(r.days, 2);
    assert_eq!(r.per_day_cents, 5_000.0);
}

#[test]
fn month_year_and_leap_boundaries() {
    let leap = calculate(3_000, d(2024, 2, 28), d(2024, 3, 1), None).unwrap();
    assert_eq!(leap.days, 3);
    let non_leap = calculate(3_000, d(2023, 2, 28), d(2023, 3, 1), None).unwrap();
    assert_eq!(non_leap.days, 2);
    let cross_year = calculate(366_000, d(2025, 12, 31), d(2026, 1, 1), None).unwrap();
    assert_eq!(cross_year.days, 2);
}

#[test]
fn target_before_purchase_is_invalid() {
    let err = calculate(10_000, d(2026, 1, 10), d(2026, 1, 9), None).unwrap_err();
    assert!(err.to_string().contains("早于购买日期"));
}

#[test]
fn no_residual_uses_full_cost() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 11), None).unwrap();
    assert_eq!(r.days, 11);
    assert_eq!(r.numerator_cents, 10_000);
    assert!((r.per_day_cents - 10_000.0 / 11.0).abs() < 1e-9);
}

#[test]
fn partial_residual_is_deducted() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 6), Some(2_500)).unwrap();
    assert_eq!(r.days, 6);
    assert_eq!(r.numerator_cents, 7_500);
    assert_eq!(r.per_day_cents, 1_250.0);
}

#[test]
fn residual_equal_to_cost_is_zero() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 6), Some(10_000)).unwrap();
    assert_eq!(r.numerator_cents, 0);
    assert_eq!(r.per_day_cents, 0.0);
}

#[test]
fn residual_above_cost_clamps_to_zero() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 6), Some(12_000)).unwrap();
    assert_eq!(r.numerator_cents, 0);
    assert_eq!(r.per_day_cents, 0.0);
}

#[test]
fn negative_cost_clamps_to_zero() {
    let r = calculate(-5_000, d(2026, 1, 1), d(2026, 1, 6), None).unwrap();
    assert_eq!(r.numerator_cents, 0);
    assert_eq!(r.per_day_cents, 0.0);
}

#[test]
fn calculate_to_today_defaults_target_to_today() {
    let t = today();
    let r = calculate_to_today(10_000, t, None).unwrap();
    assert_eq!(r.days, 1);
    assert_eq!(r.per_day_cents, 10_000.0);
}

#[test]
fn convenience_entry_matches_general_entry() {
    let t = today();
    let general = calculate(7_777, t, t, Some(777)).unwrap();
    let convenience = calculate_to_today(7_777, t, Some(777)).unwrap();
    assert_eq!(general, convenience);
}
