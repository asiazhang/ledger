//! ECB 汇率同步编排（issue #1544）：深度判据与全量 / 增量两腿分派的行为断言 +
//! 生产通道束接线证明。断言面为库内可观察的落库行与返回统计（ADR-0087 断言
//! 强度），不断言函数调用形状；接线证明例外——经本地 HTTP 服务驱动生产束
//!（[`FxSyncChannels::with_hosts`]），断言两个 ECB 文件路径的到达（删除接线即红）。
//!
//! 失败原因三态互不吞并与人工行保护经编排回归（issue #1545，spec #1540「数据源
//! 不可达 vs 该来源无数据要能分辨」）：取数网络失败 → `fx.source-unreachable`，
//! 推导零点 → `fx.source-no-data`，`fx.source-malformed` 原样透传不折算。
//!
//! 判据夹具的域时刻（账户创建日 / 交易日）经种子 + UPDATE 显式传入（种子簿记戳
//! 是 FIXED_NOW，行为输入按 ADR-0084 由测试显式给定）；取数腿覆盖字典全量币种。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::NaiveDate;
use rusqlite::Connection;

use tauri_app_lib::test_support::{
    block_on, seed_account, seed_exchange_rate_with_source, seed_fx_rate_history, seed_instrument,
};

use crate::ecb::EcbDayRates;
use crate::fx::{FX_SOURCE_ORIGIN_WEEK, FxSyncChannels, FxSyncReport, sync_fx_rates};
use crate::weekly::week_monday;

use super::{insert_lot, spawn_header_capture_server};

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

/// 汇率历史周键跨度（深度判据的可观察落库面）。
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
// 深度判据与两腿分派（issue #1544 AC，#1759 判据收敛）
// ---------------------------------------------------------------------------

/// 零痕迹 → 零请求零落库（AC）：账本没有非本位币账户与交易时，同步不碰任何
/// 通道、不落任何行——不做无谓回填。
/// 起点锚自洽（#1759）：锚 = CNY 腿起点 2005-04-01 所属 ISO 周的周一——周键口径
/// 单侧调整或锚值漂移时本测试红（锚不再是无绑定的裸字符串字面量）。
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

/// 全量腿整份灌库（#1759）：窗口判据退役——取数腿覆盖到的全部周点照常落库，
/// 最早非本位币痕迹之前的周不再被裁掉（旧判据把序列锚在痕迹前一周，历史导入
/// 永远撞「该周历史空缺」：写入需要该周汇率、汇率需要更早痕迹的死锁）。取数
/// 腿本来就整份下载，裁剪只省本地行——不为省行保留死锁判据。
#[test]
fn full_backfill_ingests_whole_document_without_window_trimming() {
    let conn = tauri_app_lib::test_support::open();
    // 非本位币账户创建日 2022-06-15：只定「有痕迹要同步」，不再定深度。
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    // 取数腿含痕迹日之前的周（2022-05-30 属更早周）——旧窗口判据会把它裁掉。
    let (mut channels, full_calls, incr_calls) = stub_channels(
        &conn,
        &[
            ("2022-05-30", 7.1), // 痕迹日之前的周：整份灌库后照常落库
            ("2022-06-06", 7.2),
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
        Some("2022-05-30"),
        "取数腿最早周照常落库（不再按窗口裁剪）"
    );
    assert_eq!(max_week.as_deref(), Some("2026-09-14"));
    assert_eq!(
        fx_point_count(&conn),
        40,
        "10 对 × 取数腿全部 4 个周（不裁剪）"
    );
    // 币种对 = 字典全量对本位币（不只「有痕迹的币种」），10 对全量落库。
    let pairs: i64 = conn
        .query_row(
            "SELECT count(DISTINCT base_code) FROM fx_rate_history",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pairs, 10, "字典 11 币种减本位币 = 10 对全量落库");
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

/// 零痕迹判据的排除面（软删行与隐藏账户）：软删账户 / 软删交易 / 隐藏账户都
/// 不算非本位币痕迹——黑洞账户（V004 种子，承接「资金账户=无」的导入交易，
/// 对用户隐藏）在每本账本必然存在，其创建日是安装时刻而非用户使用非本位币的
/// 起点；其内的导入交易不受影响（transactions 无隐藏位，仍是真实痕迹）。账本
/// 只有这些行时不触发任何取数、零落库（删除排除任一环即红）。
#[test]
fn soft_deleted_and_hidden_traces_do_not_trigger_sync() {
    let conn = tauri_app_lib::test_support::open();
    // 软删账户（2023-01-01 创建）。
    seed_foreign_account_at(&conn, "acc-del", "USD", "2023-01-01T00:00:00Z");
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-del'", [])
        .unwrap();
    // 隐藏账户（2021 创建）：排除——若未排除会触发回填。
    seed_foreign_account_at(&conn, "acc-hidden", "USD", "2021-01-01T00:00:00Z");
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hidden'", [])
        .unwrap();
    // 软删交易（2023-03-01，隐藏账户内第二只标的）：排除。
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

/// 深度判据 = 数据源起点已覆盖（#1759，替代痕迹派生窗口）：全量腿落库覆盖到
/// 起点锚（CNY 腿起点 2005-04-01 所属 ISO 周周一 2005-03-28）后，重复同步只走
/// 90 天增量、不重复拉全量（幂等，逐币种对核对）；同数据整周覆盖幂等，行数不变。
#[test]
fn repeated_sync_walks_incremental_once_source_origin_covered() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    // 取数腿含起点锚当周（2005-04-01，CNY 腿起点）与近期。
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

/// 起点未覆盖 → 全量腿再启动（#1759 判据的负向面）：首轮只灌到近期周点时
///（落库序列尚无起点锚当周行），下一轮仍判「未达深」再走全量——判据看落库
/// 序列对数据源起点的覆盖，不看「是否曾经回填过」。
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

/// 存量旧来源行（东财时代的 fx_rate_history 行，source='eastmoney'）不计入 ECB
/// 深度判据（spec #1540 / issue #1551 AC）：全字典各对只有不构成 ECB 覆盖证据的旧来源
/// 行时，深度判据仍判「未达」——全量回填照常执行，同周键的旧值被 ECB 交叉值
/// 覆盖、来源翻转为 'ecb'。把深度判据的 `source='ecb'` 过滤删掉，本用例即红：
/// 旧来源行会被误当作新来源覆盖证据，同步走增量腿、旧值原样留存。
#[test]
fn legacy_eastmoney_rows_do_not_count_as_ecb_depth_and_are_overwritten() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    // 全字典各非本位币对各种一条旧来源行（值 9.99，非 ECB 口径，周键 2022-06-06）。
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
    // 同周键（2022-06-06）被 ECB 全量回填覆盖：值 = 交叉值、来源翻转 'ecb'。
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
    // Cube 报文（与 ecb.rs 解析测试同形）：腿覆盖字典全量币种，含起点锚当周（2005-04-01，
    // CNY 腿起点）与近期多日——全量腿整份灌库，不再按窗口裁剪。
    let codes = dictionary_codes(&conn);
    let mut xml = String::from("<Cube>");
    for (date, cny) in [
        ("2005-04-01", "7.05"), // 起点锚当周（CNY 腿起点）
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

    // 首轮：全量回填，整份文件灌库（夹具最早周 2005-03-28 照常落库，不裁剪）。
    block_on(sync_fx_rates(&conn, &mut channels)).unwrap();
    let (min_week, _) = fx_week_span(&conn, "USD", "CNY");
    assert_eq!(
        min_week.as_deref(),
        Some("2005-03-28"),
        "生产路径同样整份落库（不裁剪）"
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

// ---------------------------------------------------------------------------
// 失败三态互不吞并与结果面（issue #1545，spec #1540）
// ---------------------------------------------------------------------------

/// 缺腿日快照：只给给定腿（如响应缺本位币腿——真实报文形态同构），缺腿的币种对
/// 按「该日缺失」跳过。
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

/// 两侧同源的桩通道束（与 [`stub_channels`] 同形，日序列由调用方给全集）。
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

/// 同步 op 总数（自动采集不产同步 op 的可观察面）。
fn op_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM sync_ops", [], |r| r.get(0))
        .unwrap()
}

/// 正常路径的报告面（issue #1545 结果面）：覆盖区间 / 条数 / 币种对数经编排透出
///（首轮深度未达走全量腿），当期汇率交叉方向钉值，且自动采集不产同步 op。
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
    // 交叉方向钉值：USD/CNY = CNY 腿 ÷ USD 腿（1 base = ? quote）。
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

/// 人工行保护经编排照常生效（#1543 落库单元契约在编排路径的回归面）：当期表
/// 人工行不被覆盖并计入统计。
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

/// 报文可解析但推导零点（如响应缺本位币腿）→ 报 fx.source-no-data，
/// 「该来源无数据」与网络不可达可分辨；库内两表零写入。
#[test]
fn fx_sync_reports_no_data_when_source_yields_no_usable_points() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    // 只有 USD 腿：USD→CNY 缺 CNY 腿、其余对缺两条腿——全部对无点。
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

/// 网络不可达（连接拒绝）→ 报 fx.source-unreachable，「数据源不可达」与
/// 「该来源无数据」可分辨。既有行不受影响（#1546 同款边界在本票先钉）。
#[test]
fn fx_sync_classifies_unreachable_source() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    // 127.0.0.1:1 无监听，连接立即被拒；生产束构造（有痕迹才取数，判据非 Skip）。
    let mut channels = FxSyncChannels::with_hosts(vec!["http://127.0.0.1:1".to_string()]).unwrap();

    let err = block_on(sync_fx_rates(&conn, &mut channels)).unwrap_err();

    assert!(
        err.is_code("fx.source-unreachable"),
        "应报 fx.source-unreachable，实际 {err:?}"
    );
    assert_eq!(fx_point_count(&conn), 0, "失败不落库");
}

/// 报文非预期形状（被拦截页）→ fx.source-malformed 原样透传，
/// 不折算成「不可达」——三种失败原因互不吞并。
#[test]
fn fx_sync_passes_malformed_source_through_unwrapped() {
    let conn = tauri_app_lib::test_support::open();
    seed_foreign_account_at(&conn, "acc-usd", "USD", "2022-06-15T08:00:00Z");
    let (url, _) = spawn_header_capture_server("<html>waf blocked</html>".to_string());
    let mut channels = FxSyncChannels::with_hosts(vec![url]).unwrap();

    let err = block_on(sync_fx_rates(&conn, &mut channels)).unwrap_err();

    assert!(
        err.is_code("fx.source-malformed"),
        "malformed 应原样透传，实际 {err:?}"
    );
    assert_eq!(fx_point_count(&conn), 0, "失败不落库");
}
