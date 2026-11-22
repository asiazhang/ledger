use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::NaiveDate;
use rusqlite::Connection;

use tauri_app_lib::test_support::{
    block_on, seed_account, seed_exchange_rate_with_source, seed_fx_rate_history, seed_instrument,
};

use crate::ecb::EcbDayRates;
use crate::fx::{
    FX_SOURCE_ORIGIN_WEEK, FxSyncChannels, FxSyncGate, FxSyncReport, sync_fx_rates,
    sync_fx_rates_guarded, sync_fx_rates_with_progress,
};
use crate::http::RetryConfig;
use crate::progress::FxSyncProgress;
use crate::weekly::week_monday;

use super::{insert_lot, spawn_header_capture_server};

fn dictionary_codes(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT code FROM currencies ORDER BY code")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

fn leg_rate(codes: &[String], code: &str, cny_leg: f64) -> f64 {
    match code {
        "CNY" => cny_leg,
        "EUR" => 0.9,
        _ => 1.0 + (codes.iter().position(|c| c == code).unwrap() as f64) / 10.0,
    }
}

fn day(conn: &Connection, date: &str, cny_leg: f64) -> EcbDayRates {
    let codes = dictionary_codes(conn);
    let rates = codes
        .iter()
        .map(|code| (code.clone(), leg_rate(&codes, code, cny_leg)))
        .collect::<BTreeMap<_, _>>();
    EcbDayRates {
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        rates,
    }
}

fn stub_channels(
    conn: &Connection,
    dates: &[(&str, f64)],
) -> (FxSyncChannels, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let days: Vec<EcbDayRates> = dates.iter().map(|(d, cny)| day(conn, d, *cny)).collect();
    let (full_calls, incr_calls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let channels = FxSyncChannels {
        fetch_full: {
            let days = days.clone();
            let calls = full_calls.clone();
            Box::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                super::ready(Ok(days.clone()))
            })
        },
        fetch_incremental: {
            let days = days.clone();
            let calls = incr_calls.clone();
            Box::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                super::ready(Ok(days.clone()))
            })
        },
    };
    (channels, full_calls, incr_calls)
}

fn seed_foreign_account_at(conn: &Connection, id: &str, currency: &str, created_at: &str) {
    seed_account(conn, id, &format!("账户-{id}"), "investment", currency, 0);
    conn.execute(
        "UPDATE accounts SET created_at=?1 WHERE id=?2",
        rusqlite::params![created_at, id],
    )
    .unwrap();
}

fn fx_week_span(conn: &Connection, base: &str, quote: &str) -> (Option<String>, Option<String>) {
    conn.query_row(
        "SELECT MIN(week_start), MAX(week_start) FROM fx_rate_history \
          WHERE base_code=?1 AND quote_code=?2",
        [base, quote],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

fn fx_point_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap()
}

fn fast_retry_cfg() -> RetryConfig {
    RetryConfig {
        base_backoff: std::time::Duration::from_millis(1),
        throttle_cooldown: std::time::Duration::from_millis(1),
        ..RetryConfig::production()
    }
}

#[test]
fn fx_source_origin_anchor_is_the_cny_leg_origin_week() {
    let origin = NaiveDate::parse_from_str("2005-04-01", "%Y-%m-%d").unwrap();
    assert_eq!(
        week_monday(origin).format("%Y-%m-%d").to_string(),
        FX_SOURCE_ORIGIN_WEEK,
        "起点锚 = CNY 腿起点 2005-04-01 所属 ISO 周周一（周键口径或锚漂移即红）"
    );
}

#[test]
fn no_non_native_traces_skips_sync_entirely() {
    let conn = tauri_app_lib::test_support::open();
    let (mut channels, full_calls, incr_calls) = stub_channels(&conn, &[]);

    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert_eq!(
        report,
        FxSyncReport::default(),
        "零痕迹：默认统计（未回填零落库）"
    );
    assert_eq!(full_calls.load(Ordering::SeqCst), 0, "全量通道零调用");
    assert_eq!(incr_calls.load(Ordering::SeqCst), 0, "增量通道零调用");
    assert_eq!(fx_point_count(&conn), 0, "零落库");
}

#[test]
fn full_backfill_ingests_whole_document_without_window_trimming() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (mut channels, full_calls, incr_calls) = stub_channels(
        &conn,
        &[
            ("2022-05-30", 7.1),
            ("2022-06-06", 7.2),
            ("2022-06-15", 7.3),
            ("2026-09-14", 7.4),
        ],
    );

    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert!(report.full_backfilled);
    assert_eq!(full_calls.load(Ordering::SeqCst), 1);
    assert_eq!(incr_calls.load(Ordering::SeqCst), 0, "深度未达时不走增量腿");
    let (min_week, max_week) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(
        min_week.as_deref(),
        Some("2022-05-30"),
        "取数腿最早周照常落库（不再按窗口裁剪）"
    );
    assert_eq!(max_week.as_deref(), Some("2026-09-14"));
    assert_eq!(
        fx_point_count(&conn),
        40,
        "10 对 × 取数腿全部 4 个周（不裁剪）"
    );
    let pairs: i64 = conn
        .query_row(
            "SELECT count(DISTINCT base_code) FROM fx_rate_history",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pairs, 10, "字典 11 币种减本位币 = 10 对全量落库");
    let codes = dictionary_codes(&conn);
    let usd_leg = leg_rate(&codes, "USD", 7.4);
    let (rate, source): (f64, String) = conn
        .query_row(
            "SELECT rate, source FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(source, "ecb", "自动写入标记 ECB 来源");
    assert!(
        (rate - 7.4 / usd_leg).abs() < 1e-9,
        "当期值 = 最新一天的交叉值（CNY 腿 ÷ USD 腿）"
    );
}

#[test]
fn soft_deleted_and_hidden_traces_do_not_trigger_sync() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-del", "USD", "2023-01-01T00:00:00Z");
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-del'", [])
        .unwrap();
    seed_foreign_account_at(&conn, "acc-hidden", "USD", "2021-01-01T00:00:00Z");
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hidden'", [])
        .unwrap();
    seed_instrument(&conn, "inst-del", "00001", "已删持仓", "HKD", "hk");
    insert_lot(&conn, "acc-hidden", "inst-del", "HKD");
    conn.execute(
        "UPDATE transactions SET is_deleted=1, date='2023-03-01' WHERE id='txn-acc-hidden-inst-del'",
        [],
    )
    .unwrap();

    let (mut channels, full_calls, incr_calls) = stub_channels(&conn, &[("2005-04-01", 7.1)]);
    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert_eq!(
        report,
        FxSyncReport::default(),
        "零活跃痕迹：默认统计（未回填零落库）"
    );
    assert_eq!(full_calls.load(Ordering::SeqCst), 0, "全量通道零调用");
    assert_eq!(incr_calls.load(Ordering::SeqCst), 0, "增量通道零调用");
    assert_eq!(fx_point_count(&conn), 0, "零落库");
}

#[test]
fn repeated_sync_walks_incremental_once_source_origin_covered() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (mut channels, full_calls, incr_calls) = stub_channels(
        &conn,
        &[
            ("2005-04-01", 7.1),
            ("2022-06-06", 7.2),
            ("2026-09-14", 7.4),
        ],
    );

    let first = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    assert!(first.full_backfilled, "首刷：全量回填");
    assert_eq!(full_calls.load(Ordering::SeqCst), 1);
    let (min_week, _) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(
        min_week.as_deref(),
        Some("2005-03-28"),
        "起点锚当周即最早周"
    );
    let rows_after_first = fx_point_count(&conn);

    let second = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    assert!(!second.full_backfilled, "起点已覆盖：第二轮不走全量");
    assert_eq!(full_calls.load(Ordering::SeqCst), 1, "全量通道不重复拉取");
    assert_eq!(incr_calls.load(Ordering::SeqCst), 1, "第二轮只走增量腿");
    assert_eq!(
        fx_point_count(&conn),
        rows_after_first,
        "增量同数据整周覆盖，行数不变"
    );
    let (min_week, _) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(min_week.as_deref(), Some("2005-03-28"), "起点覆盖深度保持");
}

#[test]
fn depth_not_reached_restarts_full_backfill() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (mut channels, full_calls, incr_calls) = stub_channels(
        &conn,
        &[
            ("2022-06-06", 7.2),
            ("2022-06-15", 7.3),
            ("2026-09-14", 7.4),
        ],
    );

    let first = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    assert!(first.full_backfilled, "首刷：全量回填");

    let second = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    assert!(second.full_backfilled, "起点未覆盖：全量腿再启动");
    assert_eq!(full_calls.load(Ordering::SeqCst), 2, "每轮各拉一次全量");
    assert_eq!(incr_calls.load(Ordering::SeqCst), 0, "未达深不走增量腿");
}

#[test]
fn legacy_eastmoney_rows_do_not_count_as_ecb_depth_and_are_overwritten() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    for code in dictionary_codes(&conn) {
        if code == "CNY" {
            continue;
        }
        seed_fx_rate_history(
            &conn,
            &format!("fxh-legacy-{code}"),
            &code,
            "CNY",
            "2022-06-06",
            9.99,
        );
    }
    let (mut channels, full_calls, incr_calls) = stub_channels(
        &conn,
        &[
            ("2022-06-06", 7.2),
            ("2022-06-15", 7.3),
            ("2026-09-14", 7.4),
        ],
    );

    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert!(
        report.full_backfilled,
        "旧来源行不构成新来源的深度证据：应走全量回填"
    );
    assert_eq!(full_calls.load(Ordering::SeqCst), 1);
    assert_eq!(incr_calls.load(Ordering::SeqCst), 0);
    let codes = dictionary_codes(&conn);
    let usd_leg = leg_rate(&codes, "USD", 7.2);
    let (rate, source): (f64, String) = conn
        .query_row(
            "SELECT rate, source FROM fx_rate_history \
              WHERE base_code='USD' AND quote_code='CNY' AND week_start='2022-06-06'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(source, "ecb", "同周键覆盖后来源标记为新来源");
    assert!(
        (rate - 7.2 / usd_leg).abs() < 1e-9,
        "同周键旧值被 ECB 交叉值覆盖，实际 rate={rate}"
    );
    assert_eq!(fx_point_count(&conn), 30, "10 对 × 3 个夹具周，无重复行");
}

#[test]
fn production_bundle_hits_both_ecb_documents() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let codes = dictionary_codes(&conn);
    let mut xml = String::from("<Cube>");
    for (date, cny) in [
        ("2005-04-01", "7.05"),
        ("2022-05-30", "7.1"),
        ("2022-06-06", "7.2"),
        ("2022-06-15", "7.3"),
        ("2026-09-14", "7.4"),
    ] {
        xml.push_str(&format!(r#"<Cube time="{date}">"#));
        for (idx, code) in codes.iter().enumerate() {
            let rate = match code.as_str() {
                "CNY" => cny.to_string(),
                "EUR" => "0.9".to_string(),
                _ => format!("{:.4}", 1.0 + (idx as f64) / 10.0),
            };
            xml.push_str(&format!(r#"<Cube currency="{code}" rate="{rate}"/>"#));
        }
        xml.push_str("</Cube>");
    }
    xml.push_str("</Cube>");
    let (url, heads) = super::spawn_header_capture_server(xml);
    let mut channels = FxSyncChannels::with_hosts_on(
        vec![url],
        super::zero_pacer(),
        fast_retry_cfg(),
        fast_retry_cfg(),
    )
    .unwrap();

    block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    let (min_week, _) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(
        min_week.as_deref(),
        Some("2005-03-28"),
        "生产路径同样整份落库（不裁剪）"
    );

    block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    let heads = heads.lock().unwrap();
    assert_eq!(heads.len(), 2, "两轮各一次文件请求");
    assert!(
        heads[0].contains("/stats/eurofxref/eurofxref-hist.xml"),
        "回填打到全量历史文件，实际 {}",
        heads[0]
    );
    assert!(
        heads[1].contains("/stats/eurofxref/eurofxref-hist-90d.xml"),
        "深度达成后的增量打到 90 天文件，实际 {}",
        heads[1]
    );
}

fn partial_leg_day(date: &str, legs: &[&str]) -> EcbDayRates {
    let rates = legs
        .iter()
        .map(|code| (code.to_string(), 7.3))
        .collect::<BTreeMap<_, _>>();
    EcbDayRates {
        date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
        rates,
    }
}

fn stub_channels_with_days(days: Vec<EcbDayRates>) -> FxSyncChannels {
    FxSyncChannels {
        fetch_full: {
            let days = days.clone();
            Box::new(move || super::ready(Ok(days.clone())))
        },
        fetch_incremental: {
            let days = days.clone();
            Box::new(move || super::ready(Ok(days.clone())))
        },
    }
}

fn op_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM sync_ops", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn fx_sync_reports_persist_stats_and_produces_no_sync_ops() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (mut channels, _, _) = stub_channels(
        &conn,
        &[
            ("2022-06-06", 7.2),
            ("2022-06-15", 7.3),
            ("2026-09-14", 7.4),
        ],
    );

    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert_eq!(report.persist.pairs, 10, "字典 11 币种减本位币 = 10 对");
    assert_eq!(report.persist.points, 30, "10 对 × 3 个夹具周");
    assert_eq!(report.persist.earliest.as_deref(), Some("2022-06-06"));
    assert_eq!(report.persist.latest.as_deref(), Some("2026-09-14"));
    assert_eq!(report.persist.manual_protected, 0);
    let codes = dictionary_codes(&conn);
    let usd_leg = leg_rate(&codes, "USD", 7.4);
    let (rate, source): (f64, String) = conn
        .query_row(
            "SELECT rate, source FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(source, "ecb");
    assert!((rate - 7.4 / usd_leg).abs() < 1e-12, "实际 rate={rate}");
    assert_eq!(op_count(&conn), 0, "自动采集不产同步 op");
}

#[test]
fn fx_sync_keeps_manual_rows_through_the_orchestration() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    seed_exchange_rate_with_source(&conn, "HKD", "CNY", 0.8800, "2026-01-01", "manual");
    let (mut channels, _, _) = stub_channels(&conn, &[("2022-06-06", 7.2), ("2026-09-14", 7.4)]);

    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert_eq!(report.persist.manual_protected, 1, "人工行计入保护统计");
    let (rate, priced_at): (f64, String) = conn
        .query_row(
            "SELECT rate, priced_at FROM exchange_rates WHERE base_code='HKD' AND quote_code='CNY'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(rate, 0.8800, "人工录入值不被覆盖");
    assert_eq!(priced_at, "2026-01-01", "人工行采集日期不被触碰");
}

#[test]
fn fx_sync_reports_no_data_when_source_yields_no_usable_points() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let mut channels = stub_channels_with_days(vec![partial_leg_day("2026-09-18", &["USD"])]);

    let err = block_on(sync_fx_rates(&conn, &mut channels)).unwrap_err();

    assert!(
        err.is_code("fx.source-no-data"),
        "应报 fx.source-no-data，实际 {err:?}"
    );
    assert_eq!(fx_point_count(&conn), 0, "无数据不落库");
    let exchange_rows: i64 = conn
        .query_row("SELECT count(*) FROM exchange_rates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(exchange_rows, 0, "无数据不写当期表");
}

#[test]
fn fx_sync_classifies_unreachable_source() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let mut channels = FxSyncChannels::with_hosts_on(
        vec!["http://127.0.0.1:1".to_string()],
        super::zero_pacer(),
        fast_retry_cfg(),
        fast_retry_cfg(),
    )
    .unwrap();

    let err = block_on(sync_fx_rates(&conn, &mut channels)).unwrap_err();

    assert!(
        err.is_code("fx.source-unreachable"),
        "应报 fx.source-unreachable，实际 {err:?}"
    );
    assert_eq!(fx_point_count(&conn), 0, "失败不落库");
}

#[test]
fn fx_sync_passes_malformed_source_through_unwrapped() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (url, _) = spawn_header_capture_server("<html>waf blocked</html>".to_string());
    let mut channels = FxSyncChannels::with_hosts_on(
        vec![url],
        super::zero_pacer(),
        fast_retry_cfg(),
        fast_retry_cfg(),
    )
    .unwrap();

    let err = block_on(sync_fx_rates(&conn, &mut channels)).unwrap_err();

    assert!(
        err.is_code("fx.source-malformed"),
        "malformed 应原样透传，实际 {err:?}"
    );
    assert_eq!(fx_point_count(&conn), 0, "失败不落库");
}

#[test]
fn with_progress_emits_fetching_then_persisting_with_parsed_days() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (mut channels, _, _) = stub_channels(
        &conn,
        &[
            ("2022-06-06", 7.2),
            ("2022-06-15", 7.3),
            ("2026-09-14", 7.4),
        ],
    );

    let mut stages = Vec::new();
    let report = block_on(sync_fx_rates_with_progress(
        &conn,
        &mut channels,
        &mut |progress: FxSyncProgress| stages.push(progress),
    ))
    .unwrap();

    assert!(report.full_backfilled, "深度未达走全量腿");
    assert_eq!(
        stages,
        vec![FxSyncProgress::fetching(), FxSyncProgress::persisting(3),],
        "阶段按 fetching → persisting(3 天）依次推进"
    );
}

#[test]
fn skip_plan_emits_no_stage_events() {
    let conn = tauri_app_lib::test_support::open();
    let (mut channels, full_calls, incr_calls) = stub_channels(&conn, &[]);

    let mut stages = Vec::new();
    let report = block_on(sync_fx_rates_with_progress(
        &conn,
        &mut channels,
        &mut |progress: FxSyncProgress| stages.push(progress),
    ))
    .unwrap();

    assert_eq!(report, FxSyncReport::default(), "零痕迹：默认统计");
    assert!(stages.is_empty(), "跳过路径不得发出阶段事件");
    assert_eq!(full_calls.load(Ordering::SeqCst), 0);
    assert_eq!(incr_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn guarded_second_trigger_returns_in_progress_while_first_in_flight() {
    use std::sync::Arc;

    let gate = Arc::new(FxSyncGate::new());

    let conn_first = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn_first, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let first_days: Vec<EcbDayRates> = [("2022-06-06", 7.2), ("2026-09-14", 7.4)]
        .iter()
        .map(|(date, cny)| day(&conn_first, date, *cny))
        .collect();
    let first_channels = FxSyncChannels {
        fetch_full: Box::new(move || {
            entered_tx.send(()).expect("在途通知应可送达");
            release_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("测试应放行首轮取数");
            let days = first_days.clone();
            super::ready(Ok(days))
        }),
        fetch_incremental: Box::new(|| {
            Box::pin(async {
                unreachable!("深度未达的首轮走全量腿，增量腿不应被触达")
            })
        }),
    };
    let first_gate = Arc::clone(&gate);
    let first_handle = std::thread::spawn(move || {
        let mut channels = first_channels;
        block_on(sync_fx_rates_guarded(
            &first_gate,
            &conn_first,
            &mut channels,
            &mut |_| {},
        ))
    });

    entered_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("首轮同步应到达全量取数点");
    let conn_second = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn_second, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (mut second_channels, second_full_calls, _) =
        stub_channels(&conn_second, &[("2022-06-06", 7.2)]);
    let err = block_on(sync_fx_rates_guarded(
        &gate,
        &conn_second,
        &mut second_channels,
        &mut |_| {},
    ))
    .unwrap_err();
    assert!(
        err.is_code("fx.sync-in-progress"),
        "第二路应立即报 fx.sync-in-progress，实际 {err:?}"
    );
    assert_eq!(
        second_full_calls.load(Ordering::SeqCst),
        0,
        "撞车即返：第二路不得触达取数"
    );
    assert_eq!(fx_point_count(&conn_second), 0, "撞车即返：第二路零落库");

    release_tx.send(()).expect("放行应成功");
    let first = first_handle.join().expect("首轮线程应正常结束").unwrap();
    assert!(first.full_backfilled, "首轮按判据走全量回填");

    let conn_third = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn_third, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (mut third_channels, _, _) = stub_channels(&conn_third, &[("2022-06-06", 7.2)]);
    let third = block_on(sync_fx_rates_guarded(
        &gate,
        &conn_third,
        &mut third_channels,
        &mut |_| {},
    ))
    .unwrap();
    assert!(third.full_backfilled, "门释放后新触发照常执行");
}
