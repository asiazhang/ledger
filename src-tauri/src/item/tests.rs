//! `item::cost` 接缝的单元测试（issue #114 / spec #113）。
//!
//! 断言模块外部行为：日历天数（含起止日）的定义、同日 / 目标日早于购买日 /
//! 跨月跨年闰年等边界、残值 ≥ 成本时不产生负成本、每天成本的小数精度。

use chrono::NaiveDate;

use super::cost::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

// ---------------------------------------------------------------------------
// 日历天数（含起止日）
// ---------------------------------------------------------------------------

/// 购买当日即目标日：含起止两端的定义下为 1 天（不存在 0 天）。
#[test]
fn same_day_is_one_day() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 1), None).unwrap();
    assert_eq!(r.days, 1);
    assert_eq!(r.numerator_cents, 10_000);
    assert_eq!(r.per_day_cents, 10_000.0);
}

/// 天数含购买日与目标日两端：1/1 购买、1/2 观察 → 2 天。
#[test]
fn days_are_inclusive_of_both_ends() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 2), None).unwrap();
    assert_eq!(r.days, 2);
    assert_eq!(r.per_day_cents, 5_000.0);
}

/// 跨月、跨年与闰年边界：
/// 2024-02-28 → 2024-03-01（闰年）为 3 天；2023 同区间为 2 天。
#[test]
fn month_year_and_leap_boundaries() {
    let leap = calculate(3_000, d(2024, 2, 28), d(2024, 3, 1), None).unwrap();
    assert_eq!(leap.days, 3);
    let non_leap = calculate(3_000, d(2023, 2, 28), d(2023, 3, 1), None).unwrap();
    assert_eq!(non_leap.days, 2);
    let cross_year = calculate(366_000, d(2025, 12, 31), d(2026, 1, 1), None).unwrap();
    assert_eq!(cross_year.days, 2);
}

/// 目标日早于购买日：明确报错，不静默回绕。
#[test]
fn target_before_purchase_is_invalid() {
    let err = calculate(10_000, d(2026, 1, 10), d(2026, 1, 9), None).unwrap_err();
    assert!(err.to_string().contains("早于购买日期"));
}

// ---------------------------------------------------------------------------
// 分子口径：总成本 − 残值
// ---------------------------------------------------------------------------

/// 无残值：分子 = 总成本。
#[test]
fn no_residual_uses_full_cost() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 11), None).unwrap();
    assert_eq!(r.days, 11);
    assert_eq!(r.numerator_cents, 10_000);
    assert!((r.per_day_cents - 10_000.0 / 11.0).abs() < 1e-9);
}

/// 部分残值：分子 = 总成本 − 残值，均摊到天数。
#[test]
fn partial_residual_is_deducted() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 6), Some(2_500)).unwrap();
    assert_eq!(r.days, 6);
    assert_eq!(r.numerator_cents, 7_500);
    assert_eq!(r.per_day_cents, 1_250.0);
}

/// 残值 = 成本：分子归 0，每天成本为 0（不产生负成本）。
#[test]
fn residual_equal_to_cost_is_zero() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 6), Some(10_000)).unwrap();
    assert_eq!(r.numerator_cents, 0);
    assert_eq!(r.per_day_cents, 0.0);
}

/// 残值 > 成本（录入偏高）：分子下限 0，不输出负的每天成本。
#[test]
fn residual_above_cost_clamps_to_zero() {
    let r = calculate(10_000, d(2026, 1, 1), d(2026, 1, 6), Some(12_000)).unwrap();
    assert_eq!(r.numerator_cents, 0);
    assert_eq!(r.per_day_cents, 0.0);
}

/// 总成本为负（异常录入）同样下限 0：纯计算层不放大脏数据。
#[test]
fn negative_cost_clamps_to_zero() {
    let r = calculate(-5_000, d(2026, 1, 1), d(2026, 1, 6), None).unwrap();
    assert_eq!(r.numerator_cents, 0);
    assert_eq!(r.per_day_cents, 0.0);
}

// ---------------------------------------------------------------------------
// 入口形状
// ---------------------------------------------------------------------------

/// 便捷入口 `calculate_to_today`：摊到今天（同日购买 → 1 天全额）。
#[test]
fn calculate_to_today_defaults_target_to_today() {
    let t = today();
    let r = calculate_to_today(10_000, t, None).unwrap();
    assert_eq!(r.days, 1);
    assert_eq!(r.per_day_cents, 10_000.0);
}

/// 通用入口与便捷入口对同一目标日结果一致。
#[test]
fn convenience_entry_matches_general_entry() {
    let t = today();
    let general = calculate(7_777, t, t, Some(777)).unwrap();
    let convenience = calculate_to_today(7_777, t, Some(777)).unwrap();
    assert_eq!(general, convenience);
}
