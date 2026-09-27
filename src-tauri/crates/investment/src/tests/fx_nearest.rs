//! 就近周汇率查找（issue #1844）：窗口边界域单测——精确周命中、窗口内向前 /
//! 向后就近取最近、恰在窗口边缘（第 8 周）、窗口外缺料、正反向币对兜底、
//! 同币种恒等、等距并列取较早周。
//!
//! 断言强度（ADR-0087）：对准「事件日期拿到哪一周的哪个汇率」这一调用方可
//! 观察结果（盈亏页逐腿折算的直接输入），不对准候选周枚举顺序等内部形状。
//! 纯函数测试直接构造周键索引，不经数据库——周键与 `week_start` 生成列的
//! 恒等已由 `tests::constant_price` 的绑定测试钉住。

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::fx_nearest::nearest_week_fx_rate;

/// 2026-01 各周一：01-05、01-12、01-19……（与 as_of 测试同一批周键）。
fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("ISO 日期")
}

/// 由 (base, quote, week_start, rate) 清单构造汇率历史周键索引。
fn fx(rows: &[(&str, &str, &str, f64)]) -> HashMap<(String, String), HashMap<String, f64>> {
    let mut map: HashMap<(String, String), HashMap<String, f64>> = HashMap::new();
    for (base, quote, week, rate) in rows {
        map.entry((base.to_string(), quote.to_string()))
            .or_default()
            .insert(week.to_string(), *rate);
    }
    map
}

/// 事件所在周精确命中：事件日取所在周任意一天，命中的都是该周周一的汇率。
#[test]
fn exact_week_hit_uses_event_week_rate() {
    let history = fx(&[("USD", "CNY", "2026-01-05", 7.2)]);
    // 周中（周三）与周末（周日）同命一周，都取 01-05 周一的汇率。
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-07")),
        Some(7.2)
    );
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-11")),
        Some(7.2)
    );
}

/// 事件周缺料、更早一周有点 → 取更早一周（窗口内就近取最近，issue #1844）。
#[test]
fn nearest_earlier_week_wins_within_window() {
    let history = fx(&[
        ("USD", "CNY", "2025-12-29", 7.1), // 事件周（2026-01-05）前一周
        ("USD", "CNY", "2026-01-26", 7.4), // 事件周后两周，更远
    ]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-06")),
        Some(7.1)
    );
}

/// 更早方向窗口内无点、更晚一周有点 → 取更晚一周（就近不分方向）。
#[test]
fn nearest_later_week_wins_when_no_earlier_point() {
    let history = fx(&[("USD", "CNY", "2026-01-12", 7.3)]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-08")),
        Some(7.3)
    );
}

/// 恰在窗口边缘（第 8 周）仍在窗口内：前 8 周与后 8 周都能命中。
#[test]
fn window_edge_week_eight_hits() {
    let earlier = fx(&[("USD", "CNY", "2025-11-10", 7.05)]); // 2026-01-05 前第 8 周
    assert_eq!(
        nearest_week_fx_rate(&earlier, "USD", "CNY", date("2026-01-05")),
        Some(7.05)
    );
    let later = fx(&[("USD", "CNY", "2026-03-02", 7.15)]); // 2026-01-05 后第 8 周
    assert_eq!(
        nearest_week_fx_rate(&later, "USD", "CNY", date("2026-01-05")),
        Some(7.15)
    );
}

/// 第 9 周起已在窗口外：唯一落点在第 9 周 → 缺料信号 `None`。
#[test]
fn week_nine_is_out_of_window_missing() {
    let earlier = fx(&[("USD", "CNY", "2025-11-03", 7.05)]); // 前第 9 周
    assert_eq!(
        nearest_week_fx_rate(&earlier, "USD", "CNY", date("2026-01-05")),
        None
    );
    let later = fx(&[("USD", "CNY", "2026-03-09", 7.15)]); // 后第 9 周
    assert_eq!(
        nearest_week_fx_rate(&later, "USD", "CNY", date("2026-01-05")),
        None
    );
}

/// 窗口内没有任何点（空历史）→ 缺料信号 `None`，不回落当期汇率。
#[test]
fn empty_window_returns_missing_signal() {
    assert_eq!(
        nearest_week_fx_rate(&HashMap::new(), "USD", "CNY", date("2026-01-05")),
        None
    );
}

/// 等距并列（前后各差一周都有点）取较早一周：事件时点可得的汇率只有已
/// 过去的那一周（模块头注纪律钉住）。
#[test]
fn equidistant_gap_prefers_earlier_week() {
    let history = fx(&[
        ("USD", "CNY", "2025-12-29", 7.1), // 前一周
        ("USD", "CNY", "2026-01-12", 7.3), // 后一周
    ]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-05")),
        Some(7.1)
    );
}

/// 同币种恒等：无需任何历史即为 1，不受缺料影响（既有纪律沿用）。
#[test]
fn same_currency_identity_without_history() {
    assert_eq!(
        nearest_week_fx_rate(&HashMap::new(), "CNY", "CNY", date("2026-01-05")),
        Some(1.0)
    );
}

/// 事件周正查无点、同周反查有点 → 反查取倒数（正反向兜底纪律沿用）。
#[test]
fn reverse_pair_backfills_at_event_week() {
    let history = fx(&[("CNY", "USD", "2026-01-05", 0.25)]); // 1 CNY = 0.25 USD
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-07")),
        Some(4.0) // 1 USD = 4 CNY，反查倒数
    );
}

/// 就近周同样适用正反向兜底：事件周整周缺料，最近可用周只有反查点 →
/// 按该周反查倒数，不因正查缺位而缺料。
#[test]
fn reverse_pair_backfills_at_nearest_week() {
    let history = fx(&[("CNY", "USD", "2025-12-29", 0.5)]);
    assert_eq!(
        nearest_week_fx_rate(&history, "USD", "CNY", date("2026-01-05")),
        Some(2.0)
    );
}
