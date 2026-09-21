//! 基金净值同步共享件：净值水位窗口语义。全部离线驱动，不依赖真实网络；
//! 基金分区编排（水位增量回填的端到端语义）见 `instrument_info_sync.rs`。
//!
//! 原 lsjz 报文解析与 Referer 传播用例随东财通道退役删除（issue #1571）；
//! 新浪全历史面的解析与请求形态见 `tests/sina_fund.rs`。

use chrono::NaiveDate;

use crate::fund_nav::nav_window;

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn nav_window_first_sync_backfills_two_years() {
    // 无水位（首刷）：起点 = 今天 − 2 年（恰为同月同日，无月末钳位），终点 = 今天。
    let (start, end) = nav_window(None, date("2026-08-29"));
    assert_eq!(start, "2024-08-29");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_incremental_starts_day_after_watermark() {
    // 有水位（现价缓存的净值日期）：从水位次日起，水位当日不重拉。
    let (start, end) = nav_window(Some("2026-01-30"), date("2026-08-29"));
    assert_eq!(start, "2026-01-31");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_illegal_watermark_falls_back_to_first_sync() {
    // 水位非法（理论不可达，写入侧恒 ISO 日期）：按首刷兜底自愈，不报错。
    let (start, end) = nav_window(Some("not-a-date"), date("2026-08-29"));
    assert_eq!(start, "2024-08-29");
    assert_eq!(end, "2026-08-29");
}

#[test]
fn nav_window_boundary_watermark_near_window_start() {
    // 水位早于两年窗口：起点取水位次日（增量语义不回看两年）。
    let (start, _) = nav_window(Some("2020-01-01"), date("2026-08-29"));
    assert_eq!(start, "2020-01-02");
}
