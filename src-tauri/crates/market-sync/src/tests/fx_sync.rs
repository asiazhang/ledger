//! ECB 汇率增量同步编排（issue #1545 设置页手动入口；#1546 每日自动增量将复用
//! 同一「同步一次」单元）：会话内读币种对 → 会话外取数推导 → 会话内单事务落库。
//! 断言面为库内可观察行 + 返回报告 + 码化错误分类（ADR-0087 断言强度），不钉内部函数形状。
//!
//! 错误分类三态（spec #1540「数据源不可达 vs 该来源无数据要能分辨」）：
//! - 取数网络失败 → `fx.source-unreachable`；
//! - 报文可解析但推导零点（该来源无可用数据）→ `fx.source-no-data`；
//! - 报文非预期形状（空 / 非 XML / 截断）→ `fx.source-malformed` 原样透传，不折算成不可达。

use std::time::Duration;

use rusqlite::Connection;

use crate::http::Pacer;
use tauri_app_lib::test_support::seed_exchange_rate_with_source;

use super::http_client::spawn_http_server;
use crate::fx_sync::{collect_fx_pairs, run_fx_incremental_sync_with};

/// 按给定腿集合构造一日 ECB Cube 报文（腿覆盖字典全集时每对都有点；
/// 只给部分腿时缺腿的对按「该日缺失」跳过——与真实报文形态同构）。
fn ecb_xml_with_legs(date: &str, codes: &[String]) -> String {
    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<gesmes:Envelope xmlns:gesmes="http://www.gesmes.org/xml/2002-08-01" xmlns="http://www.ecb.int/vocabulary/2002-08-01/eurofxref"><Cube>
<Cube time="{date}">"#
    );
    for (idx, code) in codes.iter().enumerate() {
        let rate = format!("{:.4}", 1.0 + (idx as f64) / 10.0);
        xml.push_str(&format!(r#"<Cube currency="{code}" rate="{rate}"/>"#));
    }
    xml.push_str("</Cube></Cube></gesmes:Envelope>");
    xml
}

fn dictionary_codes(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT code FROM currencies ORDER BY code")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

fn fx_rows(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM fx_rate_history WHERE source='ecb'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn op_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM sync_ops", [], |r| r.get(0))
        .unwrap()
}

/// 币种对收集：字典全部非本位币币种 → 本位币（方向口径与 ExchangeRate 读路径一致），
/// 按 base 币种代码升序（报告与日志确定性）。
#[test]
fn collect_fx_pairs_lists_dictionary_against_base_in_order() {
    let conn = tauri_app_lib::test_support::open();
    let pairs = collect_fx_pairs(&conn).unwrap();
    let base = ledger_transaction::amount::default_currency_code(&conn).unwrap();

    assert_eq!(pairs.len(), 10, "种子字典 11 币种，本位币除外应有 10 对");
    assert!(
        pairs
            .iter()
            .all(|(code, quote)| code != &base && quote == &base),
        "方向恒为「字典币种 → 本位币」，实际 {pairs:?}"
    );
    let mut sorted: Vec<&str> = pairs.iter().map(|(code, _)| code.as_str()).collect();
    sorted.sort_unstable();
    let codes: Vec<&str> = pairs.iter().map(|(code, _)| code.as_str()).collect();
    assert_eq!(codes, sorted, "币种对按 base 代码升序");
}

/// 正常路径：ECB 假响应驱动一次完整同步——报告给出币种对数 / 周点数 / 覆盖区间，
/// 两表落库（历史每对一周点、当期每对一行），不产同步 op。
#[test]
fn fx_sync_persists_report_and_rows_from_ecb_response() {
    let conn = tauri_app_lib::test_support::open();
    let codes = dictionary_codes(&conn);
    let body = ecb_xml_with_legs("2026-09-18", &codes);
    let url = spawn_http_server(move |_| (200, body.clone()));
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let report = tauri::async_runtime::block_on(run_fx_incremental_sync_with(
        &conn,
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap();

    let non_base = codes.len() - 1; // 本位币自身不构成对
    assert_eq!(report.pairs, non_base, "每个非本位币对都有序列");
    assert_eq!(report.points, non_base, "单日报文每周对一点");
    assert_eq!(report.earliest.as_deref(), Some("2026-09-18"));
    assert_eq!(report.latest.as_deref(), Some("2026-09-18"));
    assert_eq!(report.manual_protected, 0);

    assert_eq!(
        fx_rows(&conn),
        non_base as i64,
        "汇率历史每对一行（周采样）"
    );
    let (rate, source): (f64, String) = conn
        .query_row(
            "SELECT rate, source FROM exchange_rates WHERE base_code='HKD' AND quote_code='CNY'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(source, "ecb", "自动写入标记 ECB 来源");
    // 交叉方向钉值：HKD/CNY = CNY 腿 ÷ HKD 腿（1 base = ? quote）。
    let hkd_leg: f64 = codes
        .iter()
        .enumerate()
        .find(|(_, c)| c.as_str() == "HKD")
        .map(|(idx, _)| 1.0 + (idx as f64) / 10.0)
        .unwrap();
    let cny_leg: f64 = codes
        .iter()
        .enumerate()
        .find(|(_, c)| c.as_str() == "CNY")
        .map(|(idx, _)| 1.0 + (idx as f64) / 10.0)
        .unwrap();
    assert!((rate - cny_leg / hkd_leg).abs() < 1e-12, "实际 rate={rate}");
    assert_eq!(op_count(&conn), 0, "自动采集不产同步 op");
}

/// 人工行保护经编排照常生效（同票回归面）：当期表人工行不被覆盖并计入统计。
#[test]
fn fx_sync_keeps_manual_rows_through_the_orchestration() {
    let conn = tauri_app_lib::test_support::open();
    seed_exchange_rate_with_source(&conn, "HKD", "CNY", 0.8800, "2026-01-01", "manual");
    let codes = dictionary_codes(&conn);
    let body = ecb_xml_with_legs("2026-09-18", &codes);
    let url = spawn_http_server(move |_| (200, body.clone()));
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let report = tauri::async_runtime::block_on(run_fx_incremental_sync_with(
        &conn,
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap();

    assert_eq!(report.manual_protected, 1, "人工行计入保护统计");
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

/// 报文可解析但推导零点（如响应缺本位币腿）→ 报 fx.source-no-data，
/// 「该来源无数据」与网络不可达可分辨；库内零写入。
#[test]
fn fx_sync_reports_no_data_when_source_yields_no_usable_points() {
    let conn = tauri_app_lib::test_support::open();
    // 只有 USD 腿：USD→CNY 缺 CNY 腿、其余对缺两条腿——全部对无点。
    let body = ecb_xml_with_legs("2026-09-18", &["USD".to_string()]);
    let url = spawn_http_server(move |_| (200, body.clone()));
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let err = tauri::async_runtime::block_on(run_fx_incremental_sync_with(
        &conn,
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap_err();

    assert!(
        err.is_code("fx.source-no-data"),
        "应报 fx.source-no-data，实际 {err:?}"
    );
    assert_eq!(fx_rows(&conn), 0, "无数据不落库");
    let exchange_rows: i64 = conn
        .query_row("SELECT count(*) FROM exchange_rates", [], |r| r.get(0))
        .unwrap();
    assert_eq!(exchange_rows, 0, "无数据不写当期表");
}

/// 网络不可达（连接拒绝）→ 报 fx.source-unreachable，「数据源不可达」与
/// 「该来源无数据」可分辨。既有行不受影响（#1546 同款边界在本票先钉）。
#[test]
fn fx_sync_classifies_unreachable_source() {
    let conn = tauri_app_lib::test_support::open();
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let err = tauri::async_runtime::block_on(run_fx_incremental_sync_with(
        &conn,
        &client,
        &mut pacer,
        // 127.0.0.1:1 无监听，连接立即被拒。
        &["http://127.0.0.1:1"],
    ))
    .unwrap_err();

    assert!(
        err.is_code("fx.source-unreachable"),
        "应报 fx.source-unreachable，实际 {err:?}"
    );
    assert_eq!(fx_rows(&conn), 0, "失败不落库");
}

/// 报文非预期形状（被拦截页）→ fx.source-malformed 原样透传，
/// 不折算成「不可达」——三种失败原因互不吞并。
#[test]
fn fx_sync_passes_malformed_source_through_unwrapped() {
    let conn = tauri_app_lib::test_support::open();
    let url = spawn_http_server(|_| (200, "<html>waf blocked</html>".into()));
    let client = reqwest::Client::new();
    let mut pacer = Pacer::new(Duration::ZERO);

    let err = tauri::async_runtime::block_on(run_fx_incremental_sync_with(
        &conn,
        &client,
        &mut pacer,
        &[url.as_str()],
    ))
    .unwrap_err();

    assert!(
        err.is_code("fx.source-malformed"),
        "malformed 应原样透传，实际 {err:?}"
    );
}
