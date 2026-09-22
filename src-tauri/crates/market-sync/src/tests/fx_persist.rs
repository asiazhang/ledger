//! ECB 汇率落库单元（issue #1543）：周采样序列 → fx_rate_history（币种对 × 周键
//! 整周覆盖幂等）+ exchange_rates（每对一行最新值）。断言面为库内可观察的落库行
//! 与返回统计（ADR-0087 断言强度），不断言函数调用形状。
//!
//! 「人工行不被自动写入覆盖」的负向条目（删除即红）：`manual_row_is_kept` 直接
//! 依赖落库单元内的人工行保护分支，删掉该分支（保护失效）本用例即红。

use chrono::NaiveDate;
use rusqlite::Connection;

use tauri_app_lib::test_support::seed_exchange_rate_with_source;

use crate::ecb::FxPairWeeklySeries;
use crate::persist::{FxPersistReport, persist_ecb_fx_series};

/// 周采样点日期（测试侧已解析为 NaiveDate，spec #1677 载体中立点集）。
fn day(date: &str) -> NaiveDate {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap()
}

/// 两点周采样序列：HKD→CNY（ECB 同日两腿交叉的产物形态，1 base = ? quote）。
fn hkd_cny_series() -> FxPairWeeklySeries {
    FxPairWeeklySeries {
        base: "HKD".to_string(),
        quote: "CNY".to_string(),
        points: vec![(day("2026-09-14"), 0.9100), (day("2026-09-21"), 0.9150)],
    }
}

fn fx_rows(conn: &Connection, base: &str, quote: &str) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM fx_rate_history WHERE base_code=?1 AND quote_code=?2",
        [base, quote],
        |r| r.get(0),
    )
    .unwrap()
}

fn current_rate(conn: &Connection, base: &str, quote: &str) -> (f64, String, Option<String>) {
    conn.query_row(
        "SELECT rate, priced_at, source FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        [base, quote],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

fn op_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM sync_ops", [], |r| r.get(0))
        .unwrap()
}

/// 幂等落库：同一序列重复落库，行数不变、周键不重复、当期汇率表每对仍只有一行；
/// 值与来源保持 ECB 口径（方向 1 base = ? quote 原样落库）。
#[test]
fn persisting_same_series_twice_is_idempotent() {
    let conn = tauri_app_lib::test_support::open();
    let series = hkd_cny_series();

    let first = persist_ecb_fx_series(&conn, std::slice::from_ref(&series)).unwrap();
    assert_eq!(
        first,
        FxPersistReport {
            pairs: 1,
            points: 2,
            earliest: Some("2026-09-14".to_string()),
            latest: Some("2026-09-21".to_string()),
            manual_protected: 0,
        }
    );

    let second = persist_ecb_fx_series(&conn, &[series]).unwrap();
    assert_eq!(second.points, 2, "重复落库统计仍为本次处理的周点数");
    assert_eq!(second.manual_protected, 0);

    assert_eq!(fx_rows(&conn, "HKD", "CNY"), 2, "两个周键各一行，不重复");
    let weeks: i64 = conn
        .query_row(
            "SELECT count(DISTINCT week_start) FROM fx_rate_history WHERE base_code='HKD' AND quote_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(weeks, 2, "周键不重复");
    assert_eq!(
        current_rate(&conn, "HKD", "CNY").0,
        0.9150,
        "当期值 = 最新一条"
    );
}

/// 整周覆盖：同周任一采样日写入落在同一周键行上（trade_date 与 rate 原样覆盖）。
#[test]
fn same_week_sample_overwrites_the_whole_week() {
    let conn = tauri_app_lib::test_support::open();
    persist_ecb_fx_series(&conn, &[hkd_cny_series()]).unwrap();

    // 同一周键（2026-09-21 所在周）换采样日与新值重落。
    let updated = FxPairWeeklySeries {
        base: "HKD".to_string(),
        quote: "CNY".to_string(),
        points: vec![(day("2026-09-22"), 0.9160)],
    };
    persist_ecb_fx_series(&conn, &[updated]).unwrap();

    assert_eq!(fx_rows(&conn, "HKD", "CNY"), 2, "同周覆盖不新增行");
    let (trade_date, rate): (String, f64) = conn
        .query_row(
            "SELECT trade_date, rate FROM fx_rate_history WHERE base_code='HKD' AND quote_code='CNY' AND week_start='2026-09-21'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(trade_date, "2026-09-22", "采样日整周覆盖");
    assert_eq!(rate, 0.9160, "汇率值随覆盖更新");
}

/// 落库不产生同步 op（自动采集不进日志）：落库前后 sync_ops 行数不变。
#[test]
fn persisting_produces_no_sync_ops() {
    let conn = tauri_app_lib::test_support::open();
    assert_eq!(op_count(&conn), 0);

    persist_ecb_fx_series(&conn, &[hkd_cny_series()]).unwrap();

    assert_eq!(op_count(&conn), 0, "自动采集落库不产生同步 op");
    assert_eq!(fx_rows(&conn, "HKD", "CNY"), 2, "落库本身照常完成");
}

/// 人工行保护（负向条目，删除即红）：既有当期汇率行 source='manual' 时整行跳过，
/// 自动写入不覆盖；同批次其他币种对照常写入。
#[test]
fn manual_row_is_kept_from_auto_overwrite() {
    let conn = tauri_app_lib::test_support::open();
    seed_exchange_rate_with_source(&conn, "HKD", "CNY", 0.8800, "2026-01-01", "manual");

    let series = hkd_cny_series();
    let usd_cny = FxPairWeeklySeries {
        base: "USD".to_string(),
        quote: "CNY".to_string(),
        points: vec![(day("2026-09-21"), 7.10)],
    };
    let report = persist_ecb_fx_series(&conn, &[series, usd_cny]).unwrap();
    assert_eq!(report.manual_protected, 1, "人工行计入保护统计");

    let (rate, source) = {
        let (rate, priced_at, source) = current_rate(&conn, "HKD", "CNY");
        assert_eq!(priced_at, "2026-01-01", "人工行的采集日期不被触碰");
        (rate, source)
    };
    assert_eq!(rate, 0.8800, "人工录入值不被自动写入覆盖");
    assert_eq!(source.as_deref(), Some("manual"), "来源标记不被改写");
    assert_eq!(
        fx_rows(&conn, "HKD", "CNY"),
        2,
        "保护只辖当期表，汇率历史照常落库"
    );

    let (usd_rate, usd_source) = {
        let (r, _, s) = current_rate(&conn, "USD", "CNY");
        (r, s)
    };
    assert_eq!(usd_rate, 7.10, "无人工行的币种对照常写入");
    assert_eq!(usd_source.as_deref(), Some("ecb"), "自动写入标记 ECB 来源");
}

/// 非人工来源的既有行（api 等）可被自动写入覆盖——保护只辖人工行。
#[test]
fn non_manual_row_is_overwritten_by_auto_write() {
    let conn = tauri_app_lib::test_support::open();
    seed_exchange_rate_with_source(&conn, "HKD", "CNY", 0.8800, "2026-01-01", "api");

    persist_ecb_fx_series(&conn, &[hkd_cny_series()]).unwrap();

    let (rate, source) = {
        let (r, _, s) = current_rate(&conn, "HKD", "CNY");
        (r, s)
    };
    assert_eq!(rate, 0.9150, "非人工行被最新一条覆盖");
    assert_eq!(source.as_deref(), Some("ecb"));
}

/// 单一事务：批内任一写入失败（未知币种触发外键约束），已写入的部分整体回滚，
/// 不留部分写入。
#[test]
fn failure_leaves_no_partial_writes() {
    let conn = tauri_app_lib::test_support::open();
    let good = hkd_cny_series();
    // base 币种不在字典：fx_rate_history 外键约束在第二个序列处失败。
    let bad = FxPairWeeklySeries {
        base: "XYZ".to_string(),
        quote: "CNY".to_string(),
        points: vec![(day("2026-09-21"), 1.0)],
    };

    let err = persist_ecb_fx_series(&conn, &[good, bad]).unwrap_err();
    assert!(
        err.to_string().contains("FOREIGN KEY"),
        "预期外键约束失败，实际 {err}"
    );

    assert_eq!(
        fx_rows(&conn, "HKD", "CNY"),
        0,
        "失败不留部分写入（历史表）"
    );
    let exchange_rows: i64 = conn
        .query_row("SELECT count(*) FROM exchange_rates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(exchange_rows, 0, "失败不留部分写入（当期表）");
}

/// 空序列与全空点集是无害 no-op：零行、零统计。
#[test]
fn empty_series_is_a_noop() {
    let conn = tauri_app_lib::test_support::open();
    let empty: Vec<FxPairWeeklySeries> = vec![];
    let report = persist_ecb_fx_series(&conn, &empty).unwrap();
    assert_eq!(report, FxPersistReport::default());

    let quoteless = FxPairWeeklySeries {
        base: "HKD".to_string(),
        quote: "CNY".to_string(),
        points: vec![],
    };
    let report = persist_ecb_fx_series(&conn, &[quoteless]).unwrap();
    assert_eq!(report, FxPersistReport::default());
    assert_eq!(fx_rows(&conn, "HKD", "CNY"), 0);
    let exchange_rows: i64 = conn
        .query_row("SELECT count(*) FROM exchange_rates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(exchange_rows, 0, "无点序列不写当期表");
}
