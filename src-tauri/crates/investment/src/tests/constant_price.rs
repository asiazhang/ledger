//! 恒定价格标的（ADR-0126 / issue #1450）的域单测：打标单点单向、建档常量价
//! 保障、周键合成序列与 SQLite 生成列的绑定恒等。
//!
//! 断言对准用户可观察结果（标记单向、净值日期为空、常量序列出数），不对准
//! 函数调用形状；「删除即变红」的读侧用例见 trend / mwr / staleness 各自文件。

use rusqlite::params;

use crate::constant_price::{
    ConstantPriceValue, ensure_constant_base_price, load_constant_prices, mark_constant_unit_price,
    week_monday, weekly_samples,
};
use crate::crud::get_instrument;
use crate::prices::{MarketPriceWrite, upsert_market_price};
use tauri_app_lib::test_support::open;

use super::common::insert_fund_instrument;

/// 给标的打上恒定标记（直接 UPDATE：打标本身的行为由本文件其余用例钉住）。
fn seed_constant(conn: &rusqlite::Connection, instrument_id: &str, cents: i64) {
    conn.execute(
        "UPDATE instruments SET constant_unit_price = ?2 WHERE id = ?1",
        params![instrument_id, cents],
    )
    .unwrap();
}

/// 打标单向（ADR-0126 决策 3）：只允许「未标记 → 标记」——重复调用不覆盖既有
/// 值、不复活已清空的净值日期；返回值如实反映是否实际写入。
#[test]
fn mark_is_one_way_and_idempotent() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");

    assert!(mark_constant_unit_price(&conn, "inst-fund", 10_000).unwrap());
    // 已标记：重复确认零写入（响应缺信号不得清空，单向）。
    assert!(!mark_constant_unit_price(&conn, "inst-fund", 10_000).unwrap());
    // 传不同值同样不覆盖（打标不是可逆赋值）。
    assert!(!mark_constant_unit_price(&conn, "inst-fund", 20_000).unwrap());
    let cents: Option<i64> = conn
        .query_row(
            "SELECT constant_unit_price FROM instruments WHERE id = 'inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cents, Some(10_000), "首标值不被后续确认覆盖");
}

/// 打标清空净值日期（ADR-0126 决策 5「水位语义对它不适用」）：既有现价缓存行
/// 的 nav_date 随打标置空并保持为空——用户可见的净值日期列自此显示为空。
#[test]
fn mark_clears_nav_date() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-fund",
            price_cents: 10_000,
            currency_code: "CNY",
            priced_at: "2026-01-30",
            nav_date: Some("2026-01-30"),
            source: Some("eastmoney"),
        },
    )
    .unwrap();

    assert!(mark_constant_unit_price(&conn, "inst-fund", 10_000).unwrap());
    let nav_date: Option<String> = conn
        .query_row(
            "SELECT nav_date FROM market_prices WHERE instrument_id = 'inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nav_date, None, "打标后净值日期列为空（水位语义退出）");
}

/// 建档常量价保障（ADR-0126 决策 5）：现价缓存行缺失时落一条常量价（净值日期
/// 恒空）；已有行零触碰——不再随同步更新（价格与净值日期都保持原样）。
#[test]
fn ensure_constant_base_price_inserts_once_and_never_touches_existing() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");

    // 行缺失：落一条 1.0000，净值日期空。
    assert!(
        ensure_constant_base_price(&conn, "inst-fund", 10_000, "CNY", "2026-01-30", "eastmoney")
            .unwrap()
    );
    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id = 'inst-fund'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (10_000, None), "建档常量价 = 1.0000、净值日期空");

    // 行已在：零触碰（不同值 / 不同净值日期都不写入）。
    assert!(
        !ensure_constant_base_price(&conn, "inst-fund", 20_000, "CNY", "2026-02-02", "eastmoney")
            .unwrap()
    );
    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id = 'inst-fund'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (10_000, None), "建档一条后不再更新");
}

/// 周键绑定测试（先例：行情同步域 `week_monday` ↔ week_start 生成列恒等）：
/// Rust 周键与 V010 的 `date(trade_date,'-6 days','weekday 1')` 逐日恒等——
/// 组合走势的常量合成行按它并入既有周分组，键口径漂移在此变红。
#[test]
fn week_monday_matches_sqlite_week_start_generated_column() {
    let conn = open();
    let mut day = chrono::NaiveDate::from_ymd_opt(2025, 12, 28).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 1, 11).unwrap();
    while day <= end {
        let sql_key: String = conn
            .query_row(
                "SELECT date(?1, '-6 days', 'weekday 1')",
                [day.format("%Y-%m-%d").to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            week_monday(day).format("%Y-%m-%d").to_string(),
            sql_key,
            "Rust 周键应与 SQLite week_start 生成列恒等"
        );
        day = day.succ_opt().unwrap();
    }
}

/// 周键采样序列（ADR-0126 决策 6「按区间周键合成」）：每周一条、周一为键，
/// 采样日取周日与今天 / 区间终点的较早者（不越过区间终点），区间为空即空表。
#[test]
fn weekly_samples_clamp_to_range_end_and_today() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap(); // 周一
    // 整周窗口：首末采样 = 窗口内的周日（未及今天）。
    let from = chrono::NaiveDate::from_ymd_opt(2026, 2, 2).unwrap(); // 周一
    let to = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(); // 周日
    let samples = weekly_samples(from, to, today);
    assert_eq!(
        samples.len(),
        4,
        "四周各一条（02-02 至 03-01 恰为四个自然周）"
    );
    assert_eq!(samples[0].0, "2026-02-02");
    assert_eq!(samples[0].1, "2026-02-08", "完整周的采样日为周日");
    assert_eq!(samples[3].1, "2026-03-01");

    // 本周（尚未到周日）：采样日被今天夹住。
    let samples = weekly_samples(today, today, today);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].1, "2026-03-09", "本周采样日不越过今天");

    // 区间终点在周中：采样日不越过终点。
    let thursday = chrono::NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
    let samples = weekly_samples(from, thursday, today);
    assert_eq!(samples.last().unwrap().1, "2026-03-05");

    // 空区间（起点晚于终点）。
    assert!(weekly_samples(to, from, today).is_empty());
}

/// 装载器只返回标记行，锚点取建档日的日历日（created_at ISO 时间戳的日期
/// 部分）——「全部」区间的常量序列从建档所在周起合成。
#[test]
fn load_constant_prices_returns_marked_rows_with_creation_anchor() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-flat", "000001", "普通基金");
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    seed_constant(&conn, "inst-const", 10_000);

    let values = load_constant_prices(&conn).unwrap();
    assert_eq!(
        values,
        vec![ConstantPriceValue {
            instrument_id: "inst-const".to_string(),
            price_cents: 10_000,
            currency_code: "CNY".to_string(),
            anchor_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        }],
        "只收标记行；锚点 = 建档日（FIXED_NOW 的日历日）"
    );
}

/// 读投影接线：标记行经标的读路径（列表 / 按 id 共用投影）派生为恒定价格
/// 通道——「价格来源」列据此显示恒定价格（删除投影列或派生输入即红）。
#[test]
fn marked_instrument_reads_as_constant_channel() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");
    assert_eq!(
        get_instrument(&conn, "inst-fund").unwrap().price_channel,
        crate::PriceChannel::FundNav,
        "未标记的 6 位基金仍是净值通道"
    );
    seed_constant(&conn, "inst-fund", 10_000);
    assert_eq!(
        get_instrument(&conn, "inst-fund").unwrap().price_channel,
        crate::PriceChannel::Constant,
        "恒定单位价格在场即恒定价格通道（当且仅当该列有值）"
    );
}
