//! 手动报价（issue #291 / ADR-0036）：核心接缝 `record_manual_price` 的两落点
//! 语义与信号证据归一化——现价缓存 upsert + 价格历史周采样幂等覆盖（复用
//! 既有周键与整周覆盖语义）、回填早于最新点的旧价只沉淀历史不动现价（最新点
//! 映像规则）、来源标记 manual、校验拦截。信号证据经
//! [`ManualPriceResult::any_written`] 归一化（任一落点实际写入），「是否发」的
//! 判定单点在 signals 映射（ADR-0044 / issue #333），模块单测锁定；端到端行为
//! 由 BDD 场景承载。

use rusqlite::params;

use crate::investment::manual_price::record_manual_price;
use crate::investment::{InstrumentInput, InstrumentType, ManualPriceInput, ManualPriceResult};

use super::common::setup_db;

fn quote_input(instrument_id: &str, date: &str, price_cents: i64) -> ManualPriceInput {
    ManualPriceInput {
        instrument_id: instrument_id.to_string(),
        date: date.to_string(),
        price_cents,
    }
}

fn quote(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    date: &str,
    price_cents: i64,
) -> ManualPriceResult {
    record_manual_price(conn, &quote_input(instrument_id, date, price_cents)).expect("录价失败")
}

/// 插入指定类型的标的行（手动报价消费的字典形态：类型/来源任意，币种随行）。
fn insert_instrument(conn: &rusqlite::Connection, id: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES (?1,?1,?2,'测试标的',?3,'unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test','manual')",
        params![id, kind, currency],
    )
    .unwrap();
}

/// 现价行：(price_cents, currency_code, priced_at, nav_date, source)。
type PriceRow = (i64, String, String, Option<String>, Option<String>);

fn price_row(conn: &rusqlite::Connection, instrument_id: &str) -> Option<PriceRow> {
    conn.query_row(
        "SELECT price_cents, currency_code, priced_at, nav_date, source FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        },
    )
    .ok()
}

/// 价格历史行按周键查询（week_start 生成列与既有周键同口径）：
/// (trade_date, price_cents, currency_code, source)。
fn week_point(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    any_day_in_week: &str,
) -> Option<(String, i64, String, String)> {
    conn.query_row(
        "SELECT trade_date, price_cents, currency_code, source FROM price_history \
         WHERE instrument_id=?1 AND week_start = date(?2,'-6 days','weekday 1')",
        params![instrument_id, any_day_in_week],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .ok()
}

fn history_count(conn: &rusqlite::Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM price_history WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// 信号证据归一化（生产者清单再添一处，ADR-0031 模式 → #333 判定归一化 ADR-0044）：
// 两落点结果经 `any_written` 归一化为 PriceWritten 证据，发射判定单点在 signals 映射。
// ---------------------------------------------------------------------------

#[test]
fn price_written_evidence_pins_all_outcome_shapes() {
    use crate::signals::{Signal, WriteEvidence as E, WriteOp as Op, signals_for};

    fn assert_signals(actual: &[Signal], expected: &[Signal]) {
        assert_eq!(actual, expected);
    }
    let signals = |r: &ManualPriceResult| {
        signals_for(Op::RecordManualPrice, E::PriceWritten(r.any_written()))
    };

    // 任一落点实际写入即发；两落点均未写入（零写入）不广播——失效信号的本义是「数据变了」。
    assert_signals(
        signals(&ManualPriceResult {
            history_written: true,
            current_price_written: true,
        }),
        &[Signal::PricesChanged],
    );
    // 回填旧价：只沉淀历史、不动现价，仍是实际写入 → 广播。
    assert_signals(
        signals(&ManualPriceResult {
            history_written: true,
            current_price_written: false,
        }),
        &[Signal::PricesChanged],
    );
    // 零写入不广播。
    assert_signals(
        signals(&ManualPriceResult {
            history_written: false,
            current_price_written: false,
        }),
        &[],
    );
}

// ---------------------------------------------------------------------------
// 两落点写入
// ---------------------------------------------------------------------------

#[test]
fn quote_writes_both_landings_with_manual_source() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-1", "other", "CNY");

    let outcome = quote(&conn, "inst-1", "2026-08-28", 13180);

    assert!(outcome.history_written);
    assert!(outcome.current_price_written);
    // 落点一：现价缓存 upsert，来源 manual、priced_at = 报价日、无净值日期语义。
    let price = price_row(&conn, "inst-1").expect("现价缓存应有行");
    assert_eq!(price.0, 13180);
    assert_eq!(price.1, "CNY");
    assert_eq!(price.2, "2026-08-28");
    assert_eq!(price.3, None, "手动落价无净值日期语义");
    assert_eq!(price.4.as_deref(), Some("manual"));
    // 落点二：价格历史周采样，同周键、来源 manual、币种随标的字典。
    let point = week_point(&conn, "inst-1", "2026-08-28").expect("价格历史应有周点");
    assert_eq!(point.0, "2026-08-28");
    assert_eq!(point.1, 13180);
    assert_eq!(point.2, "CNY");
    assert_eq!(point.3, "manual");
}

#[test]
fn quote_reuses_instrument_currency_and_is_idempotent_per_week() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-hkd", "other", "HKD");

    quote(&conn, "inst-hkd", "2026-08-24", 10000);
    quote(&conn, "inst-hkd", "2026-08-24", 10500);

    // 同日重复报价：仍各一行（现价缓存单行 + 周采样单行），后写覆盖先写。
    assert_eq!(history_count(&conn, "inst-hkd"), 1);
    let point = week_point(&conn, "inst-hkd", "2026-08-24").expect("周点应存在");
    assert_eq!(point.1, 10500);
    assert_eq!(price_row(&conn, "inst-hkd").unwrap().0, 10500);
}

// ---------------------------------------------------------------------------
// 同周后写覆盖先写（整周覆盖幂等）
// ---------------------------------------------------------------------------

#[test]
fn same_week_later_quote_overwrites_whole_week_and_moves_current_price() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-2", "other", "CNY");

    // 同一周（周一 08-24 与周四 08-27）：整周覆盖，仅一条周点，trade_date 随后写。
    quote(&conn, "inst-2", "2026-08-24", 10000);
    let outcome = quote(&conn, "inst-2", "2026-08-27", 12000);

    assert!(outcome.history_written);
    assert!(
        outcome.current_price_written,
        "同周后写仍为最新点，现价随之更新"
    );
    assert_eq!(history_count(&conn, "inst-2"), 1, "同周至多一条周点");
    let point = week_point(&conn, "inst-2", "2026-08-24").expect("周点应存在");
    assert_eq!(point.0, "2026-08-27", "整周覆盖：trade_date 随后写");
    assert_eq!(point.1, 12000);
    // 现价 = 最新历史点即时映像：随后写更新。
    let price = price_row(&conn, "inst-2").expect("现价缓存应有行");
    assert_eq!(price.0, 12000);
    assert_eq!(price.2, "2026-08-27");
}

// ---------------------------------------------------------------------------
// 回填早于最新价格点：只沉淀历史、不动现价（最新点映像规则）
// ---------------------------------------------------------------------------

#[test]
fn backfill_older_than_latest_settles_history_only() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-3", "other", "CNY");

    // 先录今天价，再回填早于最新点的旧价。
    quote(&conn, "inst-3", "2026-08-28", 12000);
    let outcome = quote(&conn, "inst-3", "2026-08-05", 10000);

    assert!(outcome.history_written);
    assert!(!outcome.current_price_written, "回填旧价不改变现价");
    assert_eq!(history_count(&conn, "inst-3"), 2, "旧价沉淀为独立周点");
    let backfilled = week_point(&conn, "inst-3", "2026-08-05").expect("回填周点应存在");
    assert_eq!(backfilled.1, 10000);
    // 现价保持最新点的映像，纹丝不动。
    let price = price_row(&conn, "inst-3").expect("现价缓存应有行");
    assert_eq!(price.0, 12000);
    assert_eq!(price.2, "2026-08-28");
}

#[test]
fn backfill_same_week_overwrite_of_old_point_still_keeps_current_price() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-4", "other", "CNY");

    quote(&conn, "inst-4", "2026-08-28", 12000);
    quote(&conn, "inst-4", "2026-08-05", 10000);
    // 修正旧周点（同周后写覆盖先写）：仍早于最新点，现价纹丝不动。
    let outcome = quote(&conn, "inst-4", "2026-08-06", 11000);

    assert!(outcome.history_written);
    assert!(!outcome.current_price_written);
    assert_eq!(history_count(&conn, "inst-4"), 2, "同周覆盖不产生新行");
    let corrected = week_point(&conn, "inst-4", "2026-08-05").expect("旧周点应存在");
    assert_eq!(corrected.0, "2026-08-06", "整周覆盖：trade_date 随后写");
    assert_eq!(corrected.1, 11000);
    let price = price_row(&conn, "inst-4").expect("现价缓存应有行");
    assert_eq!(price.0, 12000, "回填与修正旧价均不动现价");
    assert_eq!(price.2, "2026-08-28");
}

#[test]
fn quote_newer_than_latest_moves_current_price_forward() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-5", "other", "CNY");

    quote(&conn, "inst-5", "2026-08-05", 10000);
    let outcome = quote(&conn, "inst-5", "2026-08-28", 13180);

    assert!(
        outcome.current_price_written,
        "新报价成为最新点，现价随之更新"
    );
    let price = price_row(&conn, "inst-5").expect("现价缓存应有行");
    assert_eq!(price.0, 13180);
    assert_eq!(price.2, "2026-08-28");
    assert_eq!(history_count(&conn, "inst-5"), 2);
}

// ---------------------------------------------------------------------------
// 校验与守卫
// ---------------------------------------------------------------------------

#[test]
fn quote_rejects_non_positive_price() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-6", "other", "CNY");

    for bad in [0, -1] {
        let err = record_manual_price(&conn, &quote_input("inst-6", "2026-08-28", bad))
            .expect_err("非正价格应被拒绝");
        assert!(
            err.to_string().contains("价格必须大于 0"),
            "实际错误：{err}"
        );
    }
    assert_eq!(history_count(&conn, "inst-6"), 0, "校验失败不落库");
    assert!(price_row(&conn, "inst-6").is_none());
}

#[test]
fn quote_rejects_malformed_date() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-7", "other", "CNY");

    for bad in ["2026/08/28", "not-a-date", "", "2026-13-40"] {
        let err = record_manual_price(&conn, &quote_input("inst-7", bad, 10000))
            .expect_err("非法日期应被拒绝");
        assert!(err.to_string().contains("日期格式"), "实际错误：{err}");
    }
    assert_eq!(history_count(&conn, "inst-7"), 0, "校验失败不落库");
}

#[test]
fn quote_canonicalizes_lenient_date_input() {
    let conn = setup_db();
    insert_instrument(&conn, "inst-7b", "other", "CNY");

    // 非补零输入按解析结果规范化落库（week_start 生成列要求 canonical ISO 串）。
    let outcome = quote(&conn, "inst-7b", "2026-8-8", 10000);
    assert!(outcome.history_written);
    let point = week_point(&conn, "inst-7b", "2026-08-08").expect("周点应存在");
    assert_eq!(point.0, "2026-08-08");
}

#[test]
fn quote_rejects_unknown_instrument() {
    let conn = setup_db();

    let err = record_manual_price(&conn, &quote_input("no-such", "2026-08-28", 10000))
        .expect_err("不存在的标的应被拒绝");
    assert!(err.to_string().contains("标的不存在"), "实际错误：{err}");
    assert_eq!(history_count(&conn, "no-such"), 0);
}

#[test]
fn quote_after_upsert_reuse_keeps_working_on_existing_instrument() {
    let conn = setup_db();
    // 手动创建核心函数（（代码，类型）upsert 复用语义）建标的 → 直接录价。
    let id = crate::investment::create_instrument(
        &conn,
        InstrumentInput {
            symbol: "稳稳地幸福".into(),
            kind: InstrumentType::Other,
            name: Some("稳稳地幸福".into()),
            currency_code: "CNY".into(),
            market: None,
        },
    )
    .expect("创建标的失败");

    let outcome = quote(&conn, &id, "2026-08-28", 13180);
    assert!(outcome.history_written && outcome.current_price_written);
    let price = price_row(&conn, &id).expect("现价缓存应有行");
    assert_eq!(price.0, 13180);
    assert_eq!(price.1, "CNY", "币种随标的字典（创建时币种可选）");
}
