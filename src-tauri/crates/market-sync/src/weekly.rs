//! 周点落库深原语（spec #1677，词条「周点落库」）：价格历史（PriceHistory）与
//! 汇率历史（FxRateHistory）共用的周点写入唯一收口——「周采样 + 判新 + 整周
//! 覆盖幂等 + 事务边界」四件事吸收进一个带事务边界的落库原语。
//!
//! 入口形状：两个薄公开入口 + 私有共享核（不立 Rust trait，ADR-0103 同口味）
//! ——[`commit_price_history_weekly`] 与 [`commit_fx_rate_history_weekly`]，
//! 同参序（键、来源、点集）、同返回（`Result<bool>`，是否产生新点）。入参为
//! **载体中立的逐日报价点序列**（日期 + 报价值）：周降采样在原语内一次完成，
//! 判新与写入共享其结果，同一批数据不重算第二遍；现价刷新批量面的「当周单点」
//! 即长度为 1 的序列，同一形状。
//!
//! 判新口径 = **同周同值零写入**（含汇率历史）：降采样后的每个周点，库内同周
//! 行存在且值相同即不写（同值周不再重写，version churn 收敛）；缺行或值不同
//! 的周点整周覆盖写入，全部同值即零写入。
//!
//! 事务边界 = **嵌套感知自保障**（[`ensure_transaction`]）：调用方未持事务时
//! 原语自持「一只一事务」；调用方已持事务时并入（基金回填的现价与周点同一
//! 事务、汇率全对大事务由调用方保留外层）。原语从不在事务外写周点行。
//!
//! 置脏与价格失效信号不在原语内：原语只返回「是否产生新点」，置脏（after-commit
//! 钩子）随调用方事务提交天然发生，广播由调用方编排按返回值裁决。upsert SQL
//! 单点不动：价格历史写单点在投资域 [`ledger_investment::prices`]、汇率 upsert
//! 在本域 [`super::persist`]，原语只收口「采样 → 判新 → 覆盖 → 事务」。
//!
//! 同步窗口语义（北京日期、日更窗口、K 线窗口、两年窗口、报价代码）不是周采样，
//! 留在编排模块（[`super::incremental`]）不进本原语。
//!
//! （ADR-0113 未来区=写路径：本模块是行情同步域的落库写路径；crate 四区化逐域
//! 另票时随写路径区归位。）

use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, params};

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::prices::{price_value_to_cents, upsert_price_history};

use super::persist::upsert_fx_rate_history;

/// 该日所属 ISO 周的周一：降采样的周键，与 price_history / fx_rate_history 的
/// week_start 生成列（date(trade_date,'-6 days','weekday 1')）同口径。两侧恒等是
/// 「整周覆盖幂等」的隐式契约，由 `week_key_matches_sqlite_week_start_column`
/// 测试绑定，防止周定义单侧调整后静默漂移。
pub(crate) fn week_monday(d: NaiveDate) -> NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// 周采样核心（日线 / 基金净值 / ECB 汇率腿共用，issue #1542 起）：(日期, 数值)
/// 点按 ISO 周降采样，每周取最后一个有报价交易日的 (trade_date, 数值)。无效数值
///（≤0）跳过；整周无有效报价则该周无点；输出按日期升序。周键见 [`week_monday`]。
pub(crate) fn downsample_weekly_points<I>(points: I) -> Vec<(NaiveDate, f64)>
where
    I: IntoIterator<Item = (NaiveDate, f64)>,
{
    let mut sorted: Vec<(NaiveDate, f64)> = points
        .into_iter()
        .filter(|(_, value)| *value > 0.0)
        .collect();
    sorted.sort_by_key(|(date, _)| *date);
    let mut by_week: std::collections::BTreeMap<NaiveDate, (NaiveDate, f64)> =
        std::collections::BTreeMap::new();
    for (d, value) in sorted {
        // 升序遍历：后写入者即该周最后一个交易日。
        by_week.insert(week_monday(d), (d, value));
    }
    by_week.into_values().collect()
}

/// 价格历史周点落库（spec #1677）：载体中立的逐日报价点序列 → 周降采样 →
/// 判新（同周同值零写入）→ 整周覆盖写入，事务嵌套感知自保障。返回是否产生
/// 新点；置脏与价格失效信号由调用方编排裁决。价格刻度换算在原语内单点消费
/// 投资域 [`price_value_to_cents`]，与既有周采样口径逐位一致。
pub(crate) fn commit_price_history_weekly(
    conn: &Connection,
    instrument_id: &str,
    currency: &str,
    source: &str,
    daily_points: &[(NaiveDate, f64)],
) -> Result<bool> {
    let instrument_id = instrument_id.to_string();
    let currency = currency.to_string();
    let source = source.to_string();
    commit_weekly(
        conn,
        daily_points,
        price_value_to_cents,
        |conn| {
            // 库内既有周点按周键建索引（同周唯一行由 UNIQUE(instrument_id,
            // week_start) 保证）。行日期不可解析（理论不可达——写入侧恒为 ISO
            // 采样日）按缺失周处理：判新判「新」、整周覆盖纠正，不静默丢周。
            let mut stmt = conn.prepare(
                "SELECT trade_date, price_cents FROM price_history WHERE instrument_id = ?1",
            )?;
            let rows = stmt.query_map(params![instrument_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            let mut by_week = HashMap::new();
            for row in rows {
                let (trade_date, cents) = row?;
                if let Ok(day) = NaiveDate::parse_from_str(trade_date.trim(), "%Y-%m-%d") {
                    by_week.insert(week_monday(day), cents);
                }
            }
            Ok(by_week)
        },
        |conn, trade_date, value| {
            upsert_price_history(
                conn,
                &instrument_id,
                trade_date,
                price_value_to_cents(value),
                &currency,
                &source,
            )
        },
    )
}

/// 汇率历史周点落库（spec #1677）：载体中立的逐日报价点序列 → 周降采样 →
/// 判新（同周同值零写入——收敛前汇率落库为无条件覆盖，判新为统一引入，
/// `fx_rate_history` 的 version 列无消费方）→ 整周覆盖写入，事务嵌套感知
/// 自保障。汇率历史按可重建缓存对待（ADR-0019 修订记录）：不置脏不广播，
/// 返回值供编排记录、不接任何信号面。
pub(crate) fn commit_fx_rate_history_weekly(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    source: &str,
    daily_points: &[(NaiveDate, f64)],
) -> Result<bool> {
    let base_code = base_code.to_string();
    let quote_code = quote_code.to_string();
    let source = source.to_string();
    commit_weekly(
        conn,
        daily_points,
        |rate| rate,
        |conn| {
            // week_start 生成列即周键（与 Rust 侧 [`week_monday`] 的恒等由测试绑定）。
            let mut stmt = conn.prepare(
                "SELECT week_start, rate FROM fx_rate_history \
                 WHERE base_code = ?1 AND quote_code = ?2",
            )?;
            let rows = stmt.query_map(params![base_code, quote_code], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            let mut by_week = HashMap::new();
            for row in rows {
                let (week_start, rate) = row?;
                if let Ok(monday) = NaiveDate::parse_from_str(week_start.trim(), "%Y-%m-%d") {
                    by_week.insert(monday, rate);
                }
            }
            Ok(by_week)
        },
        |conn, trade_date, rate| {
            upsert_fx_rate_history(conn, &base_code, &quote_code, trade_date, rate, &source)
        },
    )
}

/// 两个入口的私有共享核：降采样一次 → 判新与写入共享其结果 → 同周同值的周点
/// 不写（同值周不再重写，version churn 收敛），缺行或值不同的周点逐点整周
/// 覆盖；全部同值即零写入。全程在 [`ensure_transaction`] 内（嵌套感知：调用
/// 方已持事务则并入，否则自持「一只一事务」）。
///
/// `comparable` 把载体值换到与库内列可比的口径（价格 → 刻度整数；汇率原样
/// f64）；`existing_by_week` 读库内既有周值（周键 → 可比值）；`write_point`
/// 写一个周点（整周覆盖 upsert，trade_date 为 ISO 字符串）。
fn commit_weekly<V: PartialEq>(
    conn: &Connection,
    daily_points: &[(NaiveDate, f64)],
    comparable: impl Fn(f64) -> V,
    existing_by_week: impl FnOnce(&Connection) -> Result<HashMap<NaiveDate, V>>,
    mut write_point: impl FnMut(&Connection, &str, f64) -> Result<()>,
) -> Result<bool> {
    ensure_transaction(conn, || {
        let weekly = downsample_weekly_points(daily_points.iter().copied());
        let existing = existing_by_week(conn)?;
        let new_points: Vec<(NaiveDate, f64)> = weekly
            .into_iter()
            .filter(|(date, value)| existing.get(&week_monday(*date)) != Some(&comparable(*value)))
            .collect();
        for (date, value) in &new_points {
            write_point(conn, &date.format("%Y-%m-%d").to_string(), *value)?;
        }
        Ok(!new_points.is_empty())
    })
}
