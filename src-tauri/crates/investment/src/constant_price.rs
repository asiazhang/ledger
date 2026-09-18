//! 价格恒定标的的读侧取值接缝与打标单点（ADR-0126 / issue #1450）。
//!
//! 恒定标的（货基为首个成员）的单位价格是定义性常量（货基恒 1.0000）——「问
//! 一次数据源」与「每天问一次」都不改变取值。本模块收口两件事：
//!
//! - **打标单点、单向**（决策 3）：恒定事实的唯一持久化载体是标的行的恒定
//!   单位价格列（`instruments.constant_unit_price`，V028），写入只允许
//!   「未标记 → 标记」（[`mark_constant_unit_price`]），响应缺信号（已终止
//!   基金等形态天然缺省）不清空；确认即回填的调用点在数据源自报口径三处
//!   （搜索索引类型码 → 行情接入落库半边、lsjz 收益披露声明/类型码 → 逐只
//!   刷新、详情页数据文件形态 → 历史首刷）。标记是本机对数据源事实的缓存，
//!   不产出同步 op——各设备经自己的取数面独立确认并收敛，不做一次性存量回填。
//! - **读侧一条取值接缝**（决策 6）：恒定标的的价格在**响应内**按区间周键
//!   合成，历史表不落虚拟行、存量平坦序列不再被消费。单标的走势、组合走势
//!   （市值 = 时点份额 × 常量）与资金加权收益率的边界市值三处共用本模块的
//!   装载器（[`load_constant_prices`] / [`constant_for_instrument`]）与周键
//!   序列（[`weekly_samples`]），不各自另写第二份取值口径。
//!
//! 现价缓存保留建档（或首刷）一条即不再随同步更新（决策 5）：
//! [`ensure_constant_base_price`] 只在行缺失时落一条常量价（净值日期为空——
//! 水位语义对恒定标的不适用），已有行零触碰。

use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, params};

use super::prices::{MarketPriceWrite, upsert_market_price};
use ledger_infra::db::now_iso;
use ledger_infra::error::Result;

/// 一条恒定标的的读侧取值事实：常量价（万分之一元，ADR-0038 价格刻度）、
/// 计价币种与序列锚点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantPriceValue {
    pub instrument_id: String,
    /// 恒定单位价格（万分之一元）。
    pub price_cents: i64,
    pub currency_code: String,
    /// 序列起点锚（建档日的日历日）：价格自建档起即已知，「全部」等无下界
    /// 区间的常量序列从锚点所在周起合成。
    pub anchor_date: NaiveDate,
}

/// 打标单点（ADR-0126 决策 3）：把标的标记为恒定价格标的。单向——仅未标记行
/// 写入（`constant_unit_price IS NULL` 谓词），已标记行零触碰、响应缺信号重复
/// 调用幂等；返回是否实际写入。随打标清空现价缓存的净值日期：水位语义对恒定
/// 标的不适用（决策 5），用户可见的净值日期列自此显示为空。不产出同步 op
/// ——标记是本机对数据源自报事实的缓存，各设备经自己的取数面独立确认收敛。
///
/// 调用方须在写事务内（建当/逐只刷新/首刷的落库事务）调用，两语句同事务生效。
pub fn mark_constant_unit_price(
    conn: &Connection,
    instrument_id: &str,
    constant_unit_price_cents: i64,
) -> Result<bool> {
    let now = now_iso();
    let marked = conn.execute(
        "UPDATE instruments SET constant_unit_price = ?2, updated_at = ?3 \
         WHERE id = ?1 AND constant_unit_price IS NULL",
        params![instrument_id, constant_unit_price_cents, now],
    )? > 0;
    if !marked {
        return Ok(false);
    }
    conn.execute(
        "UPDATE market_prices SET nav_date = NULL, updated_at = ?2, version = version + 1 \
         WHERE instrument_id = ?1 AND nav_date IS NOT NULL",
        params![instrument_id, now],
    )?;
    Ok(true)
}

/// 现价缓存「建档一条」保障（ADR-0126 决策 5）：恒定标的的现价缓存行缺失时落
/// 一条常量价（净值日期为空——水位语义对它不适用），已有行零触碰（不再随同步
/// 更新）。返回是否实际落行（调用方据此计入价格写入见证）。经现价缓存写入
/// 单点 [`upsert_market_price`] 执行，不另写第二份落库 SQL。
pub fn ensure_constant_base_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
    priced_at: &str,
    source: &str,
) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM market_prices WHERE instrument_id = ?1)",
        [instrument_id],
        |r| r.get(0),
    )?;
    if exists {
        return Ok(false);
    }
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            // 净值日期恒空：恒定标的没有水位语义（ADR-0126 决策 5）。
            nav_date: None,
            source: Some(source),
        },
    )?;
    Ok(true)
}

/// 装载库内全部恒定标的的读侧取值行（`constant_unit_price` 非空的行）。三个
/// 读消费面（单标的走势 / 组合走势 / 收益率边界市值）共用的取值入口。
pub fn load_constant_prices(conn: &Connection) -> Result<Vec<ConstantPriceValue>> {
    let mut stmt = conn.prepare(
        "SELECT id, constant_unit_price, currency_code, created_at FROM instruments \
         WHERE constant_unit_price IS NOT NULL ORDER BY id",
    )?;
    let rows = stmt.query_map([], map_constant_value)?;
    let mut values = Vec::new();
    for row in rows {
        values.push(row?);
    }
    Ok(values)
}

/// 单只标的的恒定取值行；未标记（或标的不存在）返回 `None`。单标的走势的
/// 分派入口——恒定标的不再读价格历史。
pub fn constant_for_instrument(
    conn: &Connection,
    instrument_id: &str,
) -> Result<Option<ConstantPriceValue>> {
    let mut stmt = conn.prepare(
        "SELECT id, constant_unit_price, currency_code, created_at FROM instruments \
         WHERE id = ?1 AND constant_unit_price IS NOT NULL",
    )?;
    let mut rows = stmt.query_map([instrument_id], map_constant_value)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// 行映射：建档时刻（UTC ISO 时间戳）截取日期部分作序列锚点；形态异常按今天
/// 兜底（建档时钟异常不影响常量本身的正确性，只影响「全部」区间的起点）。
fn map_constant_value(row: &rusqlite::Row) -> rusqlite::Result<ConstantPriceValue> {
    let created_at: String = row.get(3)?;
    let anchor_date = created_at
        .get(..10)
        .and_then(|head| NaiveDate::parse_from_str(head, "%Y-%m-%d").ok())
        .unwrap_or_else(crate::staleness::beijing_today);
    Ok(ConstantPriceValue {
        instrument_id: row.get(0)?,
        price_cents: row.get(1)?,
        currency_code: row.get(2)?,
        anchor_date,
    })
}

/// ISO 周键（周一）：Rust 形态的 `date(trade_date, '-6 days', 'weekday 1')`
/// 生成列表达式（V010），与 `price_history.week_start` 恒等——组合走势的常量
/// 合成行按它并入既有周分组，周键口径与真实价格行同源。恒等由绑定测试钉住
/// （`tests::constant_price`，先例：行情同步域的同款绑定测试）。
pub fn week_monday(date: NaiveDate) -> NaiveDate {
    date - chrono::Duration::days(date.weekday().num_days_from_monday() as i64)
}

/// 周键采样序列（ADR-0126 决策 6「按区间周键合成」）：`[from, to]` 与今天
/// 三者夹出的每周一条——周键为周一，采样交易日取该周周日与区间终点、今天的
/// 较早者（常量价逐日成立，取周内最晚可得日与时点份额口径「该周最后一个有
/// 报价交易日」对齐；采样日不越过区间终点，与真实价格行的区间裁剪同语义）。
/// 返回 `(week_start, trade_date)` 升序；区间为空返回空表。
pub fn weekly_samples(from: NaiveDate, to: NaiveDate, today: NaiveDate) -> Vec<(String, String)> {
    let end = week_monday(to).min(week_monday(today));
    let mut cursor = week_monday(from);
    let mut samples = Vec::new();
    while cursor <= end {
        let trade_date = (cursor + chrono::Duration::days(6)).min(to).min(today);
        samples.push((
            cursor.format("%Y-%m-%d").to_string(),
            trade_date.format("%Y-%m-%d").to_string(),
        ));
        cursor += chrono::Duration::days(7);
    }
    samples
}
