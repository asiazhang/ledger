//! 边界市值装载器的「≤ 截止日最新行」语义（issue #1805）：聚合下推到 SQL
//!（`MAX(trade_date) GROUP BY instrument_id` 回连）后，取数口径必须与原
//!「全量升序取回 + Rust 覆盖写」逐标的一致——截止日之后写入的行不得被吃到、
//! 截止日当日的行算数、多周序列只取最后一周。
//!
//! 断言强度（ADR-0087）：对准「边界市值用的是哪一周的价」这一用户可观察结果
//!（资金加权收益率的期初/期末现金流直接消费它），不对准 SQL 文本与行数——
//! 装载耗时（实测 23.5ms → 4.2ms、47629 行 → 242 行）不可在 CI 判定，记在
//! issue 与装载器注释里。

use chrono::NaiveDate;

use crate::as_of::AsOfValues;
use tauri_app_lib::test_support::{open, seed_instrument, seed_price_history};

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("ISO 日期")
}

/// 取每标的 ≤ 截止日的最新周点：三周序列（10/20/30 元）截止 01-15 → 用 01-12 的
/// 20 元，截止日之后的 01-19 行不得被吃到。
#[test]
fn as_of_load_takes_latest_row_at_or_before_cutoff() {
    let conn = open();
    seed_instrument(&conn, "inst-asof", "000001", "平安银行", "CNY", "unknown");
    seed_price_history(&conn, "ph-a1", "inst-asof", "2026-01-05", 1000, "CNY");
    seed_price_history(&conn, "ph-a2", "inst-asof", "2026-01-12", 2000, "CNY");
    seed_price_history(&conn, "ph-a3", "inst-asof", "2026-01-19", 3000, "CNY");

    let values = AsOfValues::load(&conn, date("2026-01-15")).unwrap();
    // 10 份 × 2000 刻度 ÷ 100 = 200 分；若误取截止日之后的 3000 → 300 分。
    assert_eq!(values.market_value(10.0, "inst-asof", "CNY"), Some(200));

    // 截止日当日的行算数（含当日，同 as-of 持仓前缀口径）。
    let values = AsOfValues::load(&conn, date("2026-01-19")).unwrap();
    assert_eq!(values.market_value(10.0, "inst-asof", "CNY"), Some(300));
}

/// 截止日之前没有任何周点 → 缺料（`None`，不以零计入），与空值语义一致。
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
