//! 投资域持仓批次（security_lots）：FIFO 消耗与结转成本单点。
//!
//! 知识归属（父 spec #1005 决策 D4 / issue #1018）：取批次（[`active_lots`]）、
//! 逐批次分摊与耗尽批次成本闭合（[`plan`]，含对卖出匹配与转换转出两条消耗记录
//! 的既往消耗 UNION）、结转成本合计（[`total_cost`]）、以及修改/删除路径的两个
//! 精确回补原语（[`restore_sell`] / [`restore_convert_out_leg`]）。卖出匹配、
//! 基金转换转出腿与两端重放共用同一消耗口径，不得各算各的（ADR-0038 / ADR-0099
//! / 确定性重放 ADR-0091 决策 2/3）。
//!
//! 守卫归属：本票「可卖出数量不足」守卫仍由调用点在取批次后调用
//! [`ensure_available_holding`]（未内化进 [`plan`]），错误构造助手
//! [`insufficient_holding_error`] 与份额容差 [`QTY_GUARD_EPSILON`] 收归本模块私有；
//! 本地 prepare 与重放计划重建的四处共用同码同文案，两端与两路径不漂移
//! （issue #1033；守卫内化进 [`plan`] 由后续票承担）。
//!
//! 不属本模块：级联/清理模板（整批清理批次与匹配、级联回补在用卖出、转换链与在用
//! 卖出占用守卫）归投资域 [`super::unwind`]（issue #1020）。

use rusqlite::Connection;

use super::prices::PRICE_UNITS_PER_FEN;
use crate::db::now_iso;
use crate::db::query::{FromRow, query_all};
use crate::error::{AppError, Result};
use crate::sync_engine::device_id;

/// 份额守卫容差（issue #1033）：f64 逐次 FIFO 扣减的累积位误差在账本量级
/// （持仓 ≪ 1e7 份）约 1e-12 ~ 1e-9，而录入粒度合同为至多四位小数（issue #416，
/// 真实超卖差异 ≥ 1e-4）——1e-6 距两侧各 3~5 个数量级，既吞掉全部位噪声、
/// 又不会放过任何真实超卖。守卫与批次耗尽判定共用同一常量，不得各写各的。
const QTY_GUARD_EPSILON: f64 = 1e-6;

/// 数量展示格式化（错误文案用）：至多 4 位小数、去尾零——与录入粒度合同
/// （issue #416 四位小数）对齐无损展示，f64 位误差（~1e-12）远在刻度以下
/// 必然消失（如 8036.109999999999 → "8036.11"）。
fn format_quantity_for_message(quantity: f64) -> String {
    format!("{quantity:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

/// 「可卖出数量不足」码化错误（buy/sell/convert 的 FIFO 守卫共用单点）：本地
/// prepare 与重放计划重建同一口径（同码同文案同插值参数），两端与两路径不漂移。
/// 数字按录入粒度合同展示，不把位噪声原文抛给用户。
fn insufficient_holding_error(total_available: f64, quantity: f64) -> AppError {
    let available_display = format_quantity_for_message(total_available);
    let quantity_display = format_quantity_for_message(quantity);
    AppError::codedp(
        "trade.insufficient-holding",
        format!("可卖出数量不足，当前持有 {available_display}，尝试卖出 {quantity_display}"),
        &[&available_display, &quantity_display],
    )
}

/// 可卖数量守卫单点（issue #1033）：裸比较 `<` 会让 f64 FIFO 扣减的累积位误差
/// 误拒真实全清仓（如持有 8036.109999999999、卖出 8036.11）。带容差判定与
/// 码化错误在此收口：本地 prepare 与重放计划重建的四处守卫共用，同码同文案，
/// 两端与两路径不漂移。本票守卫仍在调用点调用本函数（未内化进 [`plan`]）。
pub(crate) fn ensure_available_holding(total_available: f64, quantity: f64) -> Result<()> {
    if total_available + QTY_GUARD_EPSILON < quantity {
        return Err(insufficient_holding_error(total_available, quantity));
    }
    Ok(())
}

/// 某账户某标的的在用持仓批次快照（FIFO 排序键 = rowid）。
pub(crate) struct ActiveLot {
    pub(crate) id: String,
    pub(crate) remaining_quantity: f64,
    pub(crate) cost_per_unit_cents: i64,
    pub(crate) currency_code: String,
}

impl FromRow for ActiveLot {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ActiveLot {
            id: row.get(0)?,
            remaining_quantity: row.get(1)?,
            cost_per_unit_cents: row.get(2)?,
            currency_code: row.get(3)?,
        })
    }
}

/// 单批次消耗的分摊结果（卖出匹配与转换转出共用，ADR-0038 / ADR-0099）。
pub(crate) struct Consumption {
    /// 被消耗的批次；`remaining_quantity` 为**本次消耗数量**（非批次剩余）。
    pub(crate) lot: ActiveLot,
    /// 本次消耗成本（分）——卖出为匹配成本、转换为结转成本。
    pub(crate) cost_cents: i64,
}

/// FIFO 排序键 = rowid（本端插入序，先买先消耗）：不得用 created_at/id——
/// now_iso 为秒级精度，同秒建仓时随机 id tiebreak 在重放端会排出与源端
/// 不同的顺序，消耗匹配发散（确定性重放，ADR-0091 决策 2/3，issue #861）；
/// 重放端按 op 序插入批次，rowid 相对序与源端恒一致。
///
/// 卖出匹配与转换转出共用同一取数口径（同一批次的消耗次序在两种路径下必须一致）。
pub(crate) fn active_lots(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
) -> Result<Vec<ActiveLot>> {
    query_all(
        conn,
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY rowid ASC",
        rusqlite::params![account_id, instrument_id],
    )
}

/// 该批次此前已消耗成本合计（分）：卖出匹配与转换转出两条消耗记录同口径求和。
///
/// 逐条按「数量 × 批次每份成本 ÷ 换算因子」单次舍入后求和，与既有卖出匹配的
/// 重建口径一致；批次被转换后再被卖出/再转换时，已结转走的部分不被重复计入。
fn prior_consumed_cost_cents(
    conn: &Connection,
    lot_id: &str,
    cost_per_unit_cents: i64,
) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT quantity FROM security_lot_sales WHERE lot_id=?1 \
         UNION ALL \
         SELECT quantity FROM security_lot_conversions WHERE lot_id=?1",
    )?;
    let quantities = stmt
        .query_map(rusqlite::params![lot_id], |r| r.get::<_, f64>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(quantities
        .iter()
        .map(|q| (q * cost_per_unit_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64)
        .sum())
}

/// 按 FIFO 顺序把 `quantity` 分摊到 `lots` 上，并逐批次算定成本（分）。
///
/// 成本口径（卖出匹配与转换转出共用，ADR-0038 / ADR-0099）：
/// - 耗尽批次 → 买入锚点行权威金额 − 该批次此前已消耗成本之和（闭合，精确到分）；
/// - 非耗尽批次 → round(消耗数量 × 批次每份成本 ÷ 换算因子)。
pub(crate) fn plan(
    conn: &Connection,
    lots: &[ActiveLot],
    quantity: f64,
) -> Result<Vec<Consumption>> {
    let mut remaining = quantity;
    let mut out: Vec<Consumption> = Vec::new();
    for lot in lots {
        if remaining <= 0.0 {
            break;
        }
        // 耗尽判定与守卫同一容差（issue #1033）：批次剩余噪声偏高（≤ 待消耗 + 容差）
        // 时视为耗尽、整批取走——全清仓精确归零，不留尘埃批次残差（残差会让
        // remaining_quantity > 0 永远认其为持仓）；噪声偏低方向由守卫容差放行。
        let exhausts_lot = remaining + QTY_GUARD_EPSILON >= lot.remaining_quantity;
        let matched = if exhausts_lot {
            lot.remaining_quantity
        } else {
            remaining
        };
        let cost_cents = if exhausts_lot {
            let lot_total_cents: i64 = conn.query_row(
                "SELECT t.amount_cents FROM security_lots l \n                 JOIN transactions t ON t.id = l.buy_transaction_id WHERE l.id=?1",
                rusqlite::params![lot.id],
                |r| r.get(0),
            )?;
            lot_total_cents - prior_consumed_cost_cents(conn, &lot.id, lot.cost_per_unit_cents)?
        } else {
            (matched * lot.cost_per_unit_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64
        };
        out.push(Consumption {
            lot: ActiveLot {
                id: lot.id.clone(),
                remaining_quantity: matched,
                cost_per_unit_cents: lot.cost_per_unit_cents,
                currency_code: lot.currency_code.clone(),
            },
            cost_cents,
        });
        remaining -= matched;
    }
    Ok(out)
}

/// 逐批次消耗成本合计（分）：即转换的**结转成本**——行金额锚点与转入批次成本
/// 来源（ADR-0099 决策 3）。录入、产出与重放共用本单点，不得各算各的。
pub(crate) fn total_cost(consumed: &[Consumption]) -> i64 {
    consumed.iter().map(|c| c.cost_cents).sum()
}

/// 回补一笔卖出交易曾扣减的持仓并清空其卖出关联：把每笔 `security_lot_sales` 的数量
/// 加回对应 lot，再清空该卖出的 `security_lot_sales` 与 `security_transactions` 记录。
pub(crate) fn restore_sell(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let mut stmt = conn
        .prepare("SELECT lot_id, quantity FROM security_lot_sales WHERE sell_transaction_id=?1")?;
    let sales: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in sales {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(conn)?, lot_id],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_sales WHERE sell_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='sell'",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// 回补一笔转换的**转出腿**：把逐批次消耗记录的数量加回原批次剩余，再删除本转换的
/// `security_lot_conversions` 记录。
///
/// 这是删除/修改精确回补的唯一依据（转出时的 FIFO 状态在后续交易发生后不可重建，
/// ADR-0099 决策 2）——回补必须发生在清目标行之前：删 `security_transactions` 行会按
/// 外键级联清掉消耗记录（明细行归调用侧清理），数量就再也回不去了。
pub(crate) fn restore_convert_out_leg(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let mut stmt = conn
        .prepare("SELECT lot_id, quantity FROM security_lot_conversions WHERE transaction_id=?1")?;
    let consumptions: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in consumptions {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(conn)?, lot_id],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_conversions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}
