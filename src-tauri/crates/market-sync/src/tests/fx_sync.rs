//! ECB 汇率同步编排（issue #1544）：窗口判据与全量 / 增量两腿分派的行为断言 +
//! 生产通道束接线证明。断言面为库内可观察的落库行与返回统计（ADR-0087 断言
//! 强度），不断言函数调用形状；接线证明例外——经本地 HTTP 服务驱动生产束
//!（[`FxSyncChannels::with_hosts`]），断言两个 ECB 文件路径的到达（删除接线即红）。
//!
//! 判据夹具的域时刻（账户创建日 / 交易日）经种子 + UPDATE 显式传入（种子簿记戳
//! 是 FIXED_NOW，行为输入按 ADR-0084 由测试显式给定）；取数腿覆盖字典全量币种。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::NaiveDate;
use rusqlite::Connection;

use tauri_app_lib::test_support::{block_on, seed_account, seed_instrument};

use crate::ecb::EcbDayRates;
use crate::fx::{FxSyncChannels, FxSyncReport, sync_fx_rates};

use super::{insert_holding, insert_lot, spawn_header_capture_server};

// ---------------------------------------------------------------------------
// 夹具：字典腿集合、日快照与桩通道束
// ---------------------------------------------------------------------------

/// 字典币种（种子库全量，本位币 CNY 在内）。
fn dictionary_codes(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT code FROM currencies ORDER BY code")
        .unwrap();
    stmt.query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<String>>>()
        .unwrap()
}

/// 日内各腿取值（与 ecb 测试同形：CNY 腿按参数、EUR 基准腿固定、其余互异可解析）。
fn leg_rate(codes: &[String], code: &str, cny_leg: f64) -> f64 {
    match code {
        "CNY" => cny_leg,
        "EUR" => 0.9,
        _ => 1.0 + (codes.iter().position(|c| c == code).unwrap() as f64) / 10.0,
    }
}

/// 一天快照：腿覆盖字典全部币种（编排按字典全量对本位币推导，缺腿的币种对无点）。
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

/// 桩通道束：两条腿返回同一天序列、各自计数——判据分派（走哪条腿、走几次）的
/// 可观察面。断言只看计数与落库行，不断言调用形状。
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

/// 非本位币账户（簿记戳按域时刻改写，行为输入显式传入）。
fn seed_foreign_account_at(conn: &Connection, id: &str, currency: &str, created_at: &str) {
    seed_account(conn, id, &format!("账户-{id}"), "investment", currency, 0);
    conn.execute(
        "UPDATE accounts SET created_at=?1 WHERE id=?2",
        rusqlite::params![created_at, id],
    )
    .unwrap();
}

/// 汇率历史周键跨度（窗口判据的可观察落库面）。
fn fx_week_span(conn: &Connection, base: &str, quote: &str) -> (Option<String>, Option<String>) {
    conn.query_row(
        "SELECT MIN(week_start), MAX(week_start) FROM fx_rate_history \
          WHERE base_code=?1 AND quote_code=?2",
        [base, quote],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

/// 汇率历史总行数（全字典落库面）。
fn fx_point_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap()
}

// ---------------------------------------------------------------------------
// 窗口判据与两腿分派（issue #1544 AC）
// ---------------------------------------------------------------------------

/// 零痕迹 → 零请求零落库（AC）：账本没有非本位币账户与交易时，同步不碰任何
/// 通道、不落任何行——不做无谓回填。
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

/// 全量回填的窗口锚（AC 核心判据）：窗口起点 = 最早非本位币痕迹所属 ISO 周的
/// 周一再前推一周——序列自该日期之前一周起有点；与标的 K 线「近两年」窗口无关
///（痕迹 2022-06-15 早于近两年起点，序列仍补到它之前），窗口起点之前的周不落库。
#[test]
fn full_backfill_windows_at_earliest_non_native_account() {
    let conn = tauri_app_lib::test_support::open();
    // 非本位币账户创建日 2022-06-15（周三）：所属周周一 2022-06-13，前推一周
    // = 2022-06-06（窗口起点）。
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    // 取数腿含窗口起点之前（2022-05-30 属更早周）与之后的多周。
    let (mut channels, full_calls, incr_calls) = stub_channels(
        &conn,
        &[
            ("2022-05-30", 7.1), // 窗口起点之前：裁掉
            ("2022-06-06", 7.2), // 窗口起点当周（周一）
            ("2022-06-15", 7.3), // 痕迹日当周
            ("2026-09-14", 7.4), // 近期
        ],
    );

    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert!(report.full_backfilled);
    assert_eq!(full_calls.load(Ordering::SeqCst), 1);
    assert_eq!(incr_calls.load(Ordering::SeqCst), 0, "深度未达时不走增量腿");
    let (min_week, max_week) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(
        min_week.as_deref(),
        Some("2022-06-06"),
        "窗口起点周即最早周"
    );
    assert_eq!(max_week.as_deref(), Some("2026-09-14"));
    let early: i64 = conn
        .query_row(
            "SELECT count(*) FROM fx_rate_history WHERE week_start < '2022-06-06'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(early, 0, "窗口起点之前无落库点（不灌整根文件）");
    // 币种对 = 字典全量对本位币（不只「有痕迹的币种」），10 对全量落库。
    let pairs: i64 = conn
        .query_row(
            "SELECT count(DISTINCT base_code) FROM fx_rate_history",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pairs, 10, "字典 11 币种减本位币 = 10 对全量落库");
    assert_eq!(fx_point_count(&conn), 30, "10 对 × 3 个窗口内周");
    // 当期汇率表随落库更新：每对最新一条，来源标记 ECB。
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

/// 判据输入 = 非本位币账户创建日 ∪ 非本位币交易日的 MIN；软删行排除（账户与
/// 交易同规）——任一软删痕迹若未排除，窗口都会更早（负向条目，删除即红）。
#[test]
fn window_takes_min_of_traces_excluding_soft_deleted() {
    let conn = tauri_app_lib::test_support::open();
    // 软删账户（2023-01-01 创建）：排除——若未排除，窗口会锚到 2022-12-26。
    seed_foreign_account_at(&conn, "acc-del", "USD", "2023-01-01T00:00:00Z");
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-del'", [])
        .unwrap();
    // 活跃 HKD 账户（簿记戳 2026，不影响 MIN）+ 非本位币交易 2023-05-10：参与。
    insert_holding(&conn, "acc-hkd", "inst-live", "00700", "stock", "HKD", "hk");
    conn.execute(
        "UPDATE transactions SET date='2023-05-10' WHERE id='txn-acc-hkd-inst-live'",
        [],
    )
    .unwrap();
    // 软删交易（2023-03-01，同账户第二只标的）：排除——若未排除，窗口会锚到
    // 2023-02-20。
    seed_instrument(&conn, "inst-del", "00001", "已删持仓", "HKD", "hk");
    insert_lot(&conn, "acc-hkd", "inst-del", "HKD");
    conn.execute(
        "UPDATE transactions SET is_deleted=1, date='2023-03-01' WHERE id='txn-acc-hkd-inst-del'",
        [],
    )
    .unwrap();

    let (mut channels, _, _) = stub_channels(
        &conn,
        &[
            ("2023-04-03", 7.1), // 窗口起点之前：裁掉
            ("2023-05-01", 7.2), // 窗口起点当周（2023-05-10 所属周周一 2023-05-08 前推一周）
            ("2023-05-10", 7.3), // 交易日当周
            ("2026-09-14", 7.4),
        ],
    );
    let report = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();

    assert!(report.full_backfilled);
    let (min_week, _) = fx_week_span(&conn, "HKD", "CNY");
    assert_eq!(
        min_week.as_deref(),
        Some("2023-05-01"),
        "窗口锚在活跃交易日（账户创建日 2023-01-01 与软删交易 2023-03-01 被排除）"
    );
    let early: i64 = conn
        .query_row(
            "SELECT count(*) FROM fx_rate_history WHERE week_start < '2023-05-01'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(early, 0, "软删痕迹不参与窗口判据");
}

/// 深度幂等（AC3）：首次同步走全量回填；重复同步判深度已达成（逐币种对核对），
/// 只走 90 天增量、不重复拉全量；同数据整周覆盖幂等，落库行数不变。
#[test]
fn repeated_sync_skips_full_backfill_once_depth_reached() {
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
    assert_eq!(full_calls.load(Ordering::SeqCst), 1);
    let rows_after_first = fx_point_count(&conn);

    let second = block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    assert!(!second.full_backfilled, "深度已达成：第二轮不走全量");
    assert_eq!(full_calls.load(Ordering::SeqCst), 1, "全量通道不重复拉取");
    assert_eq!(incr_calls.load(Ordering::SeqCst), 1, "第二轮只走增量腿");
    assert_eq!(
        fx_point_count(&conn),
        rows_after_first,
        "增量同数据整周覆盖，行数不变"
    );
    let (min_week, _) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(min_week.as_deref(), Some("2022-06-06"), "窗口深度保持");
}

// ---------------------------------------------------------------------------
// 生产通道束接线证明（删除接线即红）
// ---------------------------------------------------------------------------

/// 生产束接线证明：经本地 HTTP 服务驱动 [`FxSyncChannels::with_hosts`]，首轮回填
/// 打到全量历史文件、深度达成后的增量打到 90 天文件——两个文件路径各自到达，
/// 通道束与取数单点的接线删除任一环即红。
#[test]
fn production_bundle_hits_both_ecb_documents() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    // Cube 报文（与 ecb.rs 解析测试同形）：腿覆盖字典全量币种，含窗口起点之前的
    // 日期（生产路径同样按窗口裁剪）。
    let codes = dictionary_codes(&conn);
    let mut xml = String::from("<Cube>");
    for (date, cny) in [
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
    let (url, heads) = spawn_header_capture_server(xml);
    let mut channels = FxSyncChannels::with_hosts(vec![url]).unwrap();

    // 首轮：全量回填，窗口裁剪生效（夹具里的 2022-05-30 属窗口前周，不落库）。
    block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    let (min_week, _) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(
        min_week.as_deref(),
        Some("2022-06-06"),
        "生产路径同样按窗口裁剪"
    );

    // 二轮：深度已达成，走 90 天增量文件。
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
