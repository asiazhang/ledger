use chrono::NaiveDate;
use rusqlite::Connection;

use crate::weekly::{commit_fx_rate_history_weekly, commit_price_history_weekly};

fn day(date: &str) -> NaiveDate {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap()
}

fn points(pairs: &[(&str, f64)]) -> Vec<(NaiveDate, f64)> {
    pairs.iter().map(|(d, v)| (day(d), *v)).collect()
}

fn seed_instrument_row(conn: &Connection, id: &str) {
    tauri_app_lib::test_support::seed_instrument(conn, id, "600001", "名称-600001", "CNY", "sh");
}

fn price_rows(conn: &Connection, instrument_id: &str) -> Vec<(String, i64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_date, price_cents, version FROM price_history \
             WHERE instrument_id = ?1 ORDER BY trade_date",
        )
        .unwrap();
    stmt.query_map([instrument_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn fx_rows(conn: &Connection, base: &str, quote: &str) -> Vec<(String, f64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_date, rate, version FROM fx_rate_history \
             WHERE base_code = ?1 AND quote_code = ?2 ORDER BY trade_date",
        )
        .unwrap();
    stmt.query_map([base, quote], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn arm_price_insert_abort(conn: &Connection, cents: i64) {
    conn.execute_batch(&format!(
        "CREATE TRIGGER fail_weekly_insert BEFORE INSERT ON price_history \
         WHEN NEW.price_cents = {cents} \
         BEGIN SELECT RAISE(ABORT, 'injected weekly write failure'); END;"
    ))
    .unwrap();
}

#[test]
fn daily_points_land_one_row_per_week_at_last_trading_day() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");

    let written = commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[
            ("2026-09-14", 1.0),
            ("2026-09-15", 1.1),
            ("2026-09-18", 1.4),
            ("2026-09-21", 2.0),
        ]),
    )
    .unwrap();

    assert!(written, "新点产生");
    assert_eq!(
        price_rows(&conn, "inst-1"),
        vec![
            ("2026-09-18".into(), 14000, 1),
            ("2026-09-21".into(), 20000, 1)
        ],
        "每周至多一条、取该周最后一个有报价交易日"
    );
}

#[test]
fn non_positive_values_are_skipped_by_sampling() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");

    let written = commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-14", 0.0), ("2026-09-15", -1.0)]),
    )
    .unwrap();
    assert!(!written, "全无效点集零写入");
    assert!(price_rows(&conn, "inst-1").is_empty());

    let written = commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[
            ("2026-09-14", 0.0),
            ("2026-09-15", -1.0),
            ("2026-09-16", 1.2),
        ]),
    )
    .unwrap();
    assert!(written);
    assert_eq!(
        price_rows(&conn, "inst-1"),
        vec![("2026-09-16".into(), 12000, 1)],
        "无效日跳过，有效日即该周采样点"
    );
}

#[test]
fn empty_points_write_nothing() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");
    let written = commit_price_history_weekly(&conn, "inst-1", "CNY", "tencent", &[]).unwrap();
    assert!(!written);
    assert!(price_rows(&conn, "inst-1").is_empty());
}

#[test]
fn same_week_same_value_writes_nothing() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");
    commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-14", 1.0), ("2026-09-18", 1.5)]),
    )
    .unwrap();

    let written = commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-14", 1.0), ("2026-09-17", 1.5)]),
    )
    .unwrap();

    assert!(!written, "同周同值零写入");
    assert_eq!(
        price_rows(&conn, "inst-1"),
        vec![("2026-09-18".into(), 15000, 1)],
        "重复获取不重写：trade_date 与 version 保持首次落库值"
    );
}

#[test]
fn same_week_different_value_overwrites_the_whole_week() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");
    commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-16", 1.5)]),
    )
    .unwrap();

    let written = commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-18", 1.8)]),
    )
    .unwrap();

    assert!(written);
    assert_eq!(
        price_rows(&conn, "inst-1"),
        vec![("2026-09-18".into(), 18000, 2)],
        "整周覆盖：同周一行、值与采样日更新"
    );
}

#[test]
fn new_week_produces_new_point_without_rewriting_old_weeks() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");
    commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-14", 1.0)]),
    )
    .unwrap();

    let written = commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-14", 1.0), ("2026-09-21", 1.1)]),
    )
    .unwrap();

    assert!(written, "任一周缺行或值不同即有新点");
    assert_eq!(
        price_rows(&conn, "inst-1"),
        vec![
            ("2026-09-14".into(), 10000, 1),
            ("2026-09-21".into(), 11000, 1)
        ],
        "旧周同值不重写，新周落一行"
    );
}

#[test]
fn failed_write_rolls_back_the_whole_batch() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");
    arm_price_insert_abort(&conn, 99_999);

    let error = commit_price_history_weekly(
        &conn,
        "inst-1",
        "CNY",
        "tencent",
        &points(&[("2026-09-14", 1.0), ("2026-09-21", 9.9999)]),
    )
    .unwrap_err();

    assert!(error.to_string().contains("injected weekly write failure"));
    assert!(
        price_rows(&conn, "inst-1").is_empty(),
        "事务失败整体回滚：第一周的写入不留存"
    );
}

#[test]
fn joins_caller_transaction_and_rolls_back_with_it() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument_row(&conn, "inst-1");

    let error = ledger_infra::db::tx_scope::ensure_transaction(&conn, || {
        commit_price_history_weekly(
            &conn,
            "inst-1",
            "CNY",
            "tencent",
            &points(&[("2026-09-14", 1.0)]),
        )?;
        Err::<(), ledger_infra::error::AppError>(ledger_infra::error::AppError::Io(
            "外层失败".into(),
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("外层失败"));
    assert!(
        price_rows(&conn, "inst-1").is_empty(),
        "原语并入外层事务：外层回滚带走周点写入"
    );
}

#[test]
fn fx_same_week_same_value_writes_nothing() {
    let conn = tauri_app_lib::test_support::open();
    let pair = &[("2026-09-14", 0.9100), ("2026-09-21", 0.9150)];
    commit_fx_rate_history_weekly(&conn, "HKD", "CNY", "ecb", &points(pair)).unwrap();

    let written = commit_fx_rate_history_weekly(
        &conn,
        "HKD",
        "CNY",
        "ecb",
        &points(&[("2026-09-15", 0.9100), ("2026-09-21", 0.9150)]),
    )
    .unwrap();

    assert!(!written);
    assert_eq!(
        fx_rows(&conn, "HKD", "CNY"),
        vec![
            ("2026-09-14".into(), 0.9100, 1),
            ("2026-09-21".into(), 0.9150, 1)
        ],
        "重复同步零重写：值与 version 保持首次落库值"
    );
}

#[test]
fn fx_new_or_changed_weeks_are_written() {
    let conn = tauri_app_lib::test_support::open();
    commit_fx_rate_history_weekly(
        &conn,
        "HKD",
        "CNY",
        "ecb",
        &points(&[("2026-09-14", 0.9100)]),
    )
    .unwrap();

    let written = commit_fx_rate_history_weekly(
        &conn,
        "HKD",
        "CNY",
        "ecb",
        &points(&[("2026-09-16", 0.9160), ("2026-09-21", 0.9150)]),
    )
    .unwrap();

    assert!(written, "同周改值 + 跨周新点");
    assert_eq!(
        fx_rows(&conn, "HKD", "CNY"),
        vec![
            ("2026-09-16".into(), 0.9160, 2),
            ("2026-09-21".into(), 0.9150, 1)
        ],
        "改值周覆盖（version 递增），新周追加，同值周零写入"
    );
}

#[test]
fn fx_empty_points_write_nothing() {
    let conn = tauri_app_lib::test_support::open();
    let written = commit_fx_rate_history_weekly(&conn, "HKD", "CNY", "ecb", &[]).unwrap();
    assert!(!written);
    assert!(fx_rows(&conn, "HKD", "CNY").is_empty());
}
