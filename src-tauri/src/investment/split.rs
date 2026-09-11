//! 份额调整（split）批次成本重述单点（ADR-0106 决策 2/3 / issue #1049）。
//!
//! split = 同一投资账户内、单标的的非现金份额变动：一笔落账把该标的**在用批次**
//! 按比例 f = (当前持仓 + Δ) / 当前持仓 重述——逐批 `remaining_quantity × f`、
//! `initial_quantity × f`、`cost_per_unit_cents ÷ f`。批次总成本在**权威口径**
//! （锚点行金额 − 既往记录消耗，见 [`lots`] 的闭合机制）下精确不变；每份成本
//! 取整产生的舍入尾差归**末批次**闭合（沿用 convert 的「尾差末腿」纪律）。
//!
//! 重述不可逆（含舍入），逐批次 before / after 快照落 `security_lot_adjustments`
//! （角色对齐 `security_lot_conversions`）——它是后续修改/删除精确回补与审计的
//! 唯一依据（本票只落审计；回补由后续票 #1051 收编）。重述本身零已实现盈亏、
//! 不写卖出匹配、不动任何账户余额（六度量系数全 0，ADR-0011 矩阵既有行）。
//!
//! 不属本模块：split 输入守卫与计划装配归 [`super::trade`]（`prepare_split`，
//! 与 buy/sell/convert 同排）；「批次被在用 split 重述后其买入/转换不可改删」的
//! 在用占用守卫归 [`super::unwind`]。

use rusqlite::Connection;

use super::lots;
use super::prices::PRICE_UNITS_PER_FEN;
use crate::db::{new_uuid, now_iso};
use crate::error::Result;
use crate::sync_engine::device_id;

/// 单批次重述快照（before / after 逐列成对，落 `security_lot_adjustments` 一行）。
pub(crate) struct LotRestatement {
    pub(crate) lot_id: String,
    pub(crate) initial_before: f64,
    pub(crate) initial_after: f64,
    pub(crate) remaining_before: f64,
    pub(crate) remaining_after: f64,
    pub(crate) cost_per_unit_before: i64,
    pub(crate) cost_per_unit_after: i64,
}

/// 在用批次重述快照行（含锚点行金额：闭合目标的权威依据）。
struct LotSnapshot {
    lot_id: String,
    initial_quantity: f64,
    remaining_quantity: f64,
    cost_per_unit_cents: i64,
    /// 锚点行金额（分）：买入行为买入权威金额、转换为结转成本——与
    /// `lots::plan` 耗尽批次的闭合锚点同源同列。
    anchor_cents: i64,
}

/// 取某账户某标的的在用批次快照（FIFO 排序键 = rowid，确定性依据同
/// `lots::active_lots`——末批次的归位随排序键确定，重放端同序可得同一尾差）。
fn snapshot_active_lots(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
) -> Result<Vec<LotSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.initial_quantity, l.remaining_quantity, l.cost_per_unit_cents, t.amount_cents \
         FROM security_lots l \
         JOIN transactions t ON t.id = l.buy_transaction_id \
         WHERE l.account_id=?1 AND l.instrument_id=?2 AND l.remaining_quantity > 0 \
         ORDER BY l.rowid ASC",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![account_id, instrument_id], |r| {
            Ok(LotSnapshot {
                lot_id: r.get(0)?,
                initial_quantity: r.get(1)?,
                remaining_quantity: r.get(2)?,
                cost_per_unit_cents: r.get(3)?,
                anchor_cents: r.get(4)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 按比例重述在用批次（纯计算，不落库）：prepare 算定、apply 原样落盘
/// （与 sell/convert 的「prepare 算定消耗、apply 落盘」同一形态，issue #1019）。
///
/// - `f = (S + Δ) / S`，S 为在用批次剩余数量合计（快照内求和）；
/// - 非末批次每份成本 = round(cpu ÷ f)（同比例稀释的独立取整）；
/// - 末批次每份成本闭合全部尾差：以「Σ 权威批次剩余成本」（锚点 − 既往记录
///   消耗，整数分、重述不动它）为目标，倒推末批次每份成本——批次总成本在权威
///   口径下精确不变，末批次吸收全部舍入尾差；
/// - 快照为空（零在用持仓）返回空表，由调用方映射为码化守卫错误。
pub(crate) fn plan_restatement(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    delta: f64,
) -> Result<Vec<LotRestatement>> {
    let lots = snapshot_active_lots(conn, account_id, instrument_id)?;
    if lots.is_empty() {
        return Ok(Vec::new());
    }
    let total_holding: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    let factor = (total_holding + delta) / total_holding;

    // 权威总成本（分）：Σ（锚点 − 既往记录消耗）——整数分，重述不改它，
    // 末批次以其为闭合目标（记录时成本口径，ADR-0106 决策 5）。
    let mut target_total_cents = 0i64;
    for lot in &lots {
        target_total_cents +=
            lot.anchor_cents - lots::prior_consumed_cost_cents(conn, &lot.lot_id)?;
    }

    let last = lots.len() - 1;
    let mut allocated_cents = 0.0f64;
    let mut out = Vec::with_capacity(lots.len());
    for (i, lot) in lots.iter().enumerate() {
        let remaining_after = lot.remaining_quantity * factor;
        let initial_after = lot.initial_quantity * factor;
        // 非末批次独立取整稀释；末批次倒推闭合：cpu = (目标 − 已分摊) × 刻度 ÷ 数量。
        let cost_per_unit_after = if i < last {
            (lot.cost_per_unit_cents as f64 / factor).round() as i64
        } else {
            let closed = ((target_total_cents as f64 - allocated_cents) * PRICE_UNITS_PER_FEN
                / remaining_after)
                .round() as i64;
            // 防御下界：Δ>0 时各批成本非负，闭合值受已分摊舍入影响至多半分级别，
            // 不应为负；病态输入（载荷伪造）落 0 由审计可见，不静默改账。
            closed.max(0)
        };
        allocated_cents += remaining_after * cost_per_unit_after as f64 / PRICE_UNITS_PER_FEN;
        out.push(LotRestatement {
            lot_id: lot.lot_id.clone(),
            initial_before: lot.initial_quantity,
            initial_after,
            remaining_before: lot.remaining_quantity,
            remaining_after,
            cost_per_unit_before: lot.cost_per_unit_cents,
            cost_per_unit_after,
        });
    }
    Ok(out)
}

/// 应用份额调整副作用（创建路径在交易行落库后调用，与行写入同事务）：
/// `security_transactions` 的 split 扩展行（quantity = Δ、price_cents 留 NULL，
/// ADR-0106 决策 3）+ 逐批次重述落盘与 `security_lot_adjustments` 审计行。
///
/// 不写 `security_lot_sales`（零已实现盈亏）、不建新批次（成本按比例重述，
/// 否决 0 成本新批次——ADR-0106 决策 2）。
pub(crate) fn write_split_side_effects(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    delta_quantity: f64,
    restated: &[LotRestatement],
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'split',?3,NULL,0)",
        rusqlite::params![id, instrument_id, delta_quantity],
    )?;
    for r in restated {
        conn.execute(
            "UPDATE security_lots SET initial_quantity=?2, remaining_quantity=?3, cost_per_unit_cents=?4, \
             updated_at=?5, version=version+1, device_id=?6 WHERE id=?1",
            rusqlite::params![
                r.lot_id,
                r.initial_after,
                r.remaining_after,
                r.cost_per_unit_after,
                now,
                device_id(conn)?
            ],
        )?;
        conn.execute(
            "INSERT INTO security_lot_adjustments \
             (id,transaction_id,lot_id,initial_quantity_before,initial_quantity_after,\
             remaining_quantity_before,remaining_quantity_after,cost_per_unit_cents_before,\
             cost_per_unit_cents_after,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                new_uuid(),
                id,
                r.lot_id,
                r.initial_before,
                r.initial_after,
                r.remaining_before,
                r.remaining_after,
                r.cost_per_unit_before,
                r.cost_per_unit_after,
                now
            ],
        )?;
    }
    Ok(())
}
