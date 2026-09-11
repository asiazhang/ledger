//! 份额调整（split）批次成本重述单点（ADR-0106 决策 2/3 / issue #1049 + #1050）。
//!
//! split = 同一投资账户内、单标的的非现金份额变动：一笔落账把该标的**在用批次**
//! 按比例 f = (当前持仓 + Δ) / 当前持仓 重述——逐批 `remaining_quantity × f`、
//! `initial_quantity × f`、`cost_per_unit_cents ÷ f`。批次总成本在**权威口径**
//! （锚点行金额 − 既往记录消耗，见 [`lots`] 的闭合机制）下精确不变；每份成本
//! 取整产生的舍入尾差归**末批次**闭合（沿用 convert 的「尾差末腿」纪律）。
//!
//! 正负两向共用同一公式：#1049 落 `+Δ`（份额折算 / 结转 / 送股，数量增加、每份
//! 成本稀释）；#1050 落 `−Δ`（缩股，数量减少、每份成本上升），缩股幅度取严
//! `|Δ| < 当前持仓`（[`shrink_not_less_than_holding_error`]）。
//!
//! 重述不可逆（含舍入），逐批次 before / after 快照落 `security_lot_adjustments`
//! （角色对齐 `security_lot_conversions`）——它是后续修改/删除精确回补与审计的
//! 唯一依据（本票只落审计；回补由后续票 #1051 收编）。重述本身零已实现盈亏、
//! 不写卖出匹配、不动任何账户余额（六度量系数全 0，ADR-0011 矩阵既有行）。
//!
//! 守卫归属：字段级输入守卫（标的 / 账户类型 / 手续费 / 金额 / 转入腿 / `Δ = 0`）
//! 与「`Δ > 0` 须有在用持仓」的错误映射归 [`super::trade`] 的 `prepare_split`；
//! **缩股不等式取严内化进 [`plan_restatement`]**——它需要与本函数同一份在用批次
//! 快照，且是准备路径与后续重放路径共用的「必须带门」单点（同 [`lots::plan`] 把
//! 可卖出数量守卫内化进批次分摊的先例，ADR-0106 决策 5/7、ADR-0091 重放确定性）。
//! 「批次被在用 split 重述后其买入/转换不可改删」的在用占用守卫归 [`super::unwind`]。

use rusqlite::Connection;

use super::lots::{self, QTY_GUARD_EPSILON, format_quantity_for_message};
use super::prices::PRICE_UNITS_PER_FEN;
use crate::db::{new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::sync_engine::device_id;

/// 「缩股幅度不得达到当前持仓」码化错误（ADR-0106 决策 1/7）：取严 `<`——等号
/// 让 f = 0、批次清零，成本凭空消失，与「份额调整恒不产生已实现盈亏」冲突。
/// 文案对准缩股语义（不借用卖出 / 超卖口径），数字按录入粒度合同展示（同
/// [`lots`] 的不足守卫），两端与两路径不漂移。
fn shrink_not_less_than_holding_error(total_holding: f64, shrink_quantity: f64) -> AppError {
    let holding_display = format_quantity_for_message(total_holding);
    let shrink_display = format_quantity_for_message(shrink_quantity);
    AppError::codedp(
        "trade.split-shrink-not-less-than-holding",
        format!("缩股幅度必须小于当前持仓，当前持有 {holding_display}，尝试缩股 {shrink_display}"),
        &[&holding_display, &shrink_display],
    )
}

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

/// 一次份额调整重述的完整结果：逐批次快照 + 两个**总量锚点**。
///
/// 总量锚点是跨端重放确定性的比对基准（ADR-0106 决策 9：重放端本地重建重述并
/// 比对最终持仓与总成本，不一致显式挂起）：源端随命令携带，重放端以本地快照
/// 独立重建后逐项比对——本地快照与源端发散（前序 op 未达、载荷被篡改）即命中
/// 码化挂起，不静默落出错误持仓与成本基础。
pub(crate) struct Restatement {
    pub(crate) lots: Vec<LotRestatement>,
    /// 重述后总持仓（Σ remaining_after）。
    pub(crate) final_quantity: f64,
    /// 批次总成本（分）：权威闭合目标 Σ（锚点 − 既往记录消耗），重述精确不变
    /// （ADR-0106 决策 4 的「批次总成本精确不变」）。
    pub(crate) total_cost_cents: i64,
}

/// 单批次重述前的快照（回补的唯一依据）：读自 `security_lot_adjustments` 的
/// `_before` 列，命名列组随结构体带语义，不靠 SELECT 位置约定。
struct LotBefore {
    lot_id: String,
    initial_quantity: f64,
    remaining_quantity: f64,
    cost_per_unit_cents: i64,
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
///
/// `before_transaction_rowid`：非 `None` 时只取**买入落账序早于该行**的批次——
/// 修改一笔份额调整时重述目标仍是它**落账那一刻在场**的批次（ADR-0106 决策 6：
/// FIFO 状态在后续交易发生后不可重建，插入序是唯一可用的界），其后才建立的批次
/// （后续买入）不受这笔历史份额调整影响。`None`（创建路径）取当前全部在用批次。
fn snapshot_active_lots(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    before_transaction_rowid: Option<i64>,
) -> Result<Vec<LotSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.initial_quantity, l.remaining_quantity, l.cost_per_unit_cents, t.amount_cents \
         FROM security_lots l \
         JOIN transactions t ON t.id = l.buy_transaction_id \
         WHERE l.account_id=?1 AND l.instrument_id=?2 AND l.remaining_quantity > 0 \
           AND (?3 IS NULL OR t.rowid < ?3) \
         ORDER BY l.rowid ASC",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![account_id, instrument_id, before_transaction_rowid],
            |r| {
                Ok(LotSnapshot {
                    lot_id: r.get(0)?,
                    initial_quantity: r.get(1)?,
                    remaining_quantity: r.get(2)?,
                    cost_per_unit_cents: r.get(3)?,
                    anchor_cents: r.get(4)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// 按比例重述在用批次（纯计算，不落库）：prepare 算定、apply 原样落盘
/// （与 sell/convert 的「prepare 算定消耗、apply 落盘」同一形态，issue #1019）。
///
/// - `f = (S + Δ) / S`，S 为在用批次剩余数量合计（快照内求和）；
/// - 缩股 `Δ < 0` 取严 `|Δ| < S`：越界（含等号，容差同 FIFO 守卫）在此码化拒绝，
///   不让 `f ≤ 0` 进入分摊（[`shrink_not_less_than_holding_error`]）；
/// - 非末批次每份成本 = round(cpu ÷ f)（同比例稀释的独立取整）；
/// - 末批次每份成本闭合全部尾差：以「Σ 权威批次剩余成本」（锚点 − 既往记录
///   消耗，整数分、重述不动它）为目标，倒推末批次每份成本——批次总成本在权威
///   口径下精确不变，末批次吸收全部舍入尾差；
/// - 快照为空（零在用持仓）且非缩股时返回空表，由调用方映射为码化守卫错误；
///   零持仓缩股在缩股守卫处即被拒绝，不返回空表。
///
/// `before_transaction_rowid` 同 [`snapshot_active_lots`]：修改路径以本行落账序为
/// 界（ADR-0106 决策 6），创建路径为 `None`。
pub(crate) fn plan_restatement(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    delta: f64,
    before_transaction_rowid: Option<i64>,
) -> Result<Restatement> {
    let lots = snapshot_active_lots(conn, account_id, instrument_id, before_transaction_rowid)?;
    let total_holding: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    // 缩股取严（ADR-0106 决策 1/7）：|Δ| 必须严格小于当前持仓——等号让 f = 0、
    // 批次成本凭空消失。零持仓缩股同被此守卫拒绝（|Δ| > 0 恒 ≥ 0）。容差与 FIFO
    // 守卫同源：f64 逐次扣减的位噪声不误拒真实合法缩股。
    if delta < 0.0 && total_holding + delta <= QTY_GUARD_EPSILON {
        return Err(shrink_not_less_than_holding_error(total_holding, -delta));
    }
    if lots.is_empty() {
        return Ok(Restatement {
            lots: Vec::new(),
            final_quantity: 0.0,
            total_cost_cents: 0,
        });
    }
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
    let final_quantity: f64 = out.iter().map(|r| r.remaining_after).sum();
    Ok(Restatement {
        lots: out,
        final_quantity,
        total_cost_cents: target_total_cents,
    })
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

/// 精确回补一笔份额调整对在用批次的重述（修改与删除路径回退的唯一依据，
/// ADR-0106 决策 3/5 / issue #1051）：按 `security_lot_adjustments` 的逐批次
/// `_before` 快照把批次写回重述前状态——数量与每份成本**含舍入一并还原**，
/// 不做近似反算（重述不可逆，审计行是唯一依据）。
///
/// 随后清空本行的重述审计（修改路径由 apply 按新输入重建、删除路径不再存续）与
/// `security_transactions` 的 split 扩展行：修改路径的 apply 会重插该扩展行，
/// 删除路径连同软删交易行一并退场；显式删审计而不只依赖扩展行的外键级联，
/// 让「先读快照、再重建」的顺序自明，不依赖 `PRAGMA foreign_keys` 状态。
///
/// 守卫（批次已被在用下游消耗则拒绝改/删）归 [`super::unwind`]，本函数假定已放行。
pub(crate) fn restore_restatement(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    // 先读全量 before 快照再落盘：读与写不交错，审计行随后的清空不影响回补依据。
    let prior: Vec<LotBefore> = {
        let mut stmt = conn.prepare(
            "SELECT lot_id, initial_quantity_before, remaining_quantity_before, \
             cost_per_unit_cents_before FROM security_lot_adjustments WHERE transaction_id=?1",
        )?;
        stmt.query_map(rusqlite::params![id], |r| {
            Ok(LotBefore {
                lot_id: r.get(0)?,
                initial_quantity: r.get(1)?,
                remaining_quantity: r.get(2)?,
                cost_per_unit_cents: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for lot in prior {
        conn.execute(
            "UPDATE security_lots SET initial_quantity=?2, remaining_quantity=?3, cost_per_unit_cents=?4, \
             updated_at=?5, version=version+1, device_id=?6 WHERE id=?1",
            rusqlite::params![
                lot.lot_id,
                lot.initial_quantity,
                lot.remaining_quantity,
                lot.cost_per_unit_cents,
                now,
                device_id(conn)?
            ],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_adjustments WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}
