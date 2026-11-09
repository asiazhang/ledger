//! 投资域持仓副作用撤销（Unwind）：守卫 → 级联/回补 → 清理模板单点。
//!
//! 知识归属（父 spec #1005 决策 D2/D3 / issue #1020）：把原先散在 [`super::trade`]
//! 里的五份变体（`revert` 的 buy 臂与 convert 臂、`release_for_delete` 的 buy 臂与
//! convert 臂、convert 修改清理）收成唯一公开动作 [`remove`]——取批次与回补原语仍归
//! [`super::lots`]，本模块只承接「撤销一笔交易对持仓的全部影响」（ADR-0097 语言）的
//! 编排模板。`trade.rs` 的 `revert` / `release_for_delete` 退化为薄委托，对外出口不变。
//!
//! 守卫语义与用户可见文案同时收口本模块（ADR-0033 决策 #4 的修订形态）：`trade.*`
//! 错误码与中文模板随守卫知识迁入，按 kind × 模式单点选择——修改入口与删除入口措辞
//! 各自单点，买入与转换的用户可见主体各自单点；行为层不再注入 `GuardMessages`。
//! 错误码与前端错误模板零变化（ADR-0050），同步重放与本地共用同一文案天然继承。
//!
//! 依赖方向：本模块单向消费 [`super::lots`] 的回补原语，不反向依赖 [`super::trade`]。

use rusqlite::Connection;

use super::lots;
use super::split;
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::TransactionKind;

/// 撤销模式：修改路径撤销旧行后重建（[`Mode::Update`]），删除路径撤销后软删交易行
/// （[`Mode::Delete`]）。两条路径的「在用」归因谓词同一，差别只在级联：
/// 删除模式把该行批次的**在用** sell 逐笔回补并返回 id 列表（供行为层各自软删留痕），
/// 修改模式因守卫已拒绝在用占用而恒返回空列表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 修改路径：守卫放行后整批清理，副作用由重建承接。
    Update,
    /// 删除路径：撤销持仓影响，级联回补在用下游卖出。
    Delete,
}

/// buy 已有部分卖出的守卫文案——修改入口措辞（ADR-0033 决策 #4 的修订形态：按入口
/// 内化、随守卫知识单点落本模块，调用方协议面不出现文案，同一入口同一文案不漂移）。
/// 删除入口曾有的 `PARTIAL_SOLD_CANNOT_DELETE`（`trade.partially-sold-delete`）
/// 随级联删除退场（issue #940 / ADR-0097）——删除不再拒绝，改为级联。
const PARTIAL_SOLD_CANNOT_UPDATE: &str = "该买入交易已有部分卖出，无法修改";
/// 上迹守卫的稳定错误码（issue #342 二期）：与文案同源单点。
const PARTIAL_SOLD_CANNOT_UPDATE_CODE: &str = "trade.partially-sold-update";

/// 转换**转入份额**已有部分卖出的守卫文案（issue #979 / ADR-0099）：谓词与买入的
/// 「在用卖出占用」同一，但用户可见主体是转换的转入份额而非买入交易——用户可见
/// 文案各自单点，不复用买入的「该买入交易…」措辞（ADR-0049 文案准确性）。
const CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE: &str = "该转换的转入份额已被后续卖出，无法修改";
const CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE_CODE: &str = "trade.convert-partially-sold-update";

/// 持仓份额已被**在用的后续转换**消耗的守卫文案（issue #978 / ADR-0099）：修改会
/// 重建持仓批次、删除会连带清空批次，两者都会抹掉后续转换的转出消耗记录（它精确
/// 回补与结转成本的唯一依据）——链式转换（一条转换单拆多腿）须从最后一腿往前处理。
/// 修改与删除两个入口措辞各自单点定义（与上方部分卖出守卫同款）。
const CONSUMED_BY_CONVERT_CANNOT_UPDATE: &str = "该交易的份额已被后续转换消耗，无法修改";
const CONSUMED_BY_CONVERT_CANNOT_UPDATE_CODE: &str = "trade.consumed-by-convert-update";
const CONSUMED_BY_CONVERT_CANNOT_DELETE: &str = "该交易的份额已被后续转换消耗，无法删除";
const CONSUMED_BY_CONVERT_CANNOT_DELETE_CODE: &str = "trade.consumed-by-convert-delete";

/// 持仓批次已被**在用的后续份额调整**重述的守卫文案（ADR-0106 决策 5 判据家族 /
/// issue #1049）：修改会重建批次、删除会连带清空批次，两者都会把重述审计
/// （security_lot_adjustments，精确回补与审计的唯一依据）一并抹掉。修改与删除
/// 两个入口措辞各自单点定义（与上方转换链守卫同款）。
const CONSUMED_BY_SPLIT_CANNOT_UPDATE: &str = "该交易的份额已被后续份额调整重述，无法修改";
const CONSUMED_BY_SPLIT_CANNOT_UPDATE_CODE: &str = "trade.consumed-by-split-update";
const CONSUMED_BY_SPLIT_CANNOT_DELETE: &str = "该交易的份额已被后续份额调整重述，无法删除";
const CONSUMED_BY_SPLIT_CANNOT_DELETE_CODE: &str = "trade.consumed-by-split-delete";

/// 份额调整行**自身**的「批次已被下游在用消耗」守卫文案（ADR-0106 决策 5 判据家族 /
/// issue #1051）：本行重述过的批次若被其**落账序之后、且未软删**的后续交易消耗
/// （后续 sell / convert）或再次重述（后一次 split），改 / 删即拒绝——精确回补会把
/// 批次数量与每份成本直接还原到本行重述前的快照，而下游消耗是按重述后的数量与每份
/// 成本结算的，冲突会篡改下游账面。修改与删除两个入口措辞各自单点定义（与上方
/// [`CONSUMED_BY_SPLIT_CANNOT_UPDATE`] 同款）。
const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE: &str =
    "该份额调整的批次已被后续交易消耗，无法修改";
const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE_CODE: &str = "trade.split-consumed-update";
const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE: &str =
    "该份额调整的批次已被后续交易消耗，无法删除";
const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE_CODE: &str = "trade.split-consumed-delete";

/// 撤销一笔 buy / sell / convert / split 交易对持仓的全部影响（修改路径与删除路径同一动作）。
///
/// 按 kind 与模式分派（ADR-0097 / ADR-0099 / ADR-0106）：
/// - sell：回补其扣减的持仓并清空卖出关联（两模式同一，无守卫）；
/// - buy：修改模式先用「在用占用」守卫拒绝（批次已被在用后续转换消耗 / 已被在用
///   sell 匹配消耗），再整批清理批次与买入明细；删除模式经转换链守卫后**级联**——
///   其持仓批次的在用 sell 逐笔回补（各自软删由行为层编排），再整批清理；
/// - convert：与 buy 同规，另在清理前回补**转出腿**（逐批次精确回补，必须先于清目标
///   行——删目标行会按外键级联删掉消耗记录）；
/// - split：其重述过的批次被在用下游消耗则拒绝改 / 删，放行后按重述审计逐批次 before
///   快照精确回补并清空扩展行（无级联语义，ADR-0106 决策 3/5）；
/// - dividend：无持仓副作用（不改数量、不写匹配），摘除其 `security_transactions`
///   扩展行即可（改 / 删两模式同一动作，issue #1078）；
/// - 其余 kind 无持仓副作用，返回空表。
///
/// 删除模式返回被级联的 sell id 列表（ADR-0097 契约：级联对象由行为层逐笔软删并
/// 各自产出 delete op 与余额刷新）；修改模式恒空。
pub fn remove(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    mode: Mode,
) -> Result<Vec<String>> {
    match kind {
        TransactionKind::Sell => {
            lots::restore_sell(conn, id)?;
            Ok(Vec::new())
        }
        TransactionKind::Buy => match mode {
            Mode::Update => {
                guard_no_convert_consumed(conn, id, Mode::Update)?;
                guard_no_split_restated(conn, id, Mode::Update)?;
                guard_no_active_sell(
                    conn,
                    id,
                    PARTIAL_SOLD_CANNOT_UPDATE_CODE,
                    PARTIAL_SOLD_CANNOT_UPDATE,
                )?;
                purge_lot_artifacts(conn, id)?;
                Ok(Vec::new())
            }
            Mode::Delete => {
                guard_no_convert_consumed(conn, id, Mode::Delete)?;
                guard_no_split_restated(conn, id, Mode::Delete)?;
                let cascaded_sell_ids = cascade_active_sells(conn, id)?;
                purge_lot_artifacts(conn, id)?;
                Ok(cascaded_sell_ids)
            }
        },
        // 转换：与 buy 同规（ADR-0099 决策 5），另回补转出腿再清转入批次与明细行。
        TransactionKind::Convert => match mode {
            Mode::Update => {
                guard_no_convert_consumed(conn, id, Mode::Update)?;
                guard_no_split_restated(conn, id, Mode::Update)?;
                guard_no_active_sell(
                    conn,
                    id,
                    CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE_CODE,
                    CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE,
                )?;
                lots::restore_convert_out_leg(conn, id)?;
                purge_lot_artifacts(conn, id)?;
                Ok(Vec::new())
            }
            Mode::Delete => {
                guard_no_convert_consumed(conn, id, Mode::Delete)?;
                guard_no_split_restated(conn, id, Mode::Delete)?;
                let cascaded_sell_ids = cascade_active_sells(conn, id)?;
                lots::restore_convert_out_leg(conn, id)?;
                purge_lot_artifacts(conn, id)?;
                Ok(cascaded_sell_ids)
            }
        },
        // 份额调整（split）：本行重述过的批次被在用下游消耗则拒绝改 / 删
        // （ADR-0106 决策 5 / issue #1051），放行后按 `security_lot_adjustments`
        // 逐批次 before 快照精确回补（[`split::restore_restatement`]）并清空扩展行。
        // 无「级联软删下游」语义——下游消耗一旦存在即挂守卫拒绝，故恒返回空表。
        TransactionKind::Split => {
            guard_no_split_downstream_consumed(conn, id, mode)?;
            split::restore_restatement(conn, id)?;
            Ok(Vec::new())
        }
        // 现金分红（dividend，issue #1078）：无持仓副作用（不改数量、不写匹配），
        // 唯一痕迹是 `security_transactions` 扩展行——改 / 删都只需摘除它，供
        // 修改路径重建或删除路径留空；两模式同一动作，无守卫、无级联。
        TransactionKind::Dividend => {
            conn.execute(
                "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='dividend'",
                rusqlite::params![id],
            )?;
            Ok(Vec::new())
        }
        // 行为层仅对 buy/sell/convert/split/dividend 调用本函数；其余 kind 无持仓副作用，no-op
        // （显式枚举保证新增 kind 时此处编译报错，而非落入兜底）。
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => Ok(Vec::new()),
    }
}

/// 级联回补：把该行持仓批次的**在用** sell 逐笔回退持仓扣减，返回被级联的 sell id
/// 列表（软删与 op 产出归行为层删除编排）。
fn cascade_active_sells(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let sell_ids = active_sell_ids_on_own_lots(conn, anchor_id)?;
    for sell_id in &sell_ids {
        lots::restore_sell(conn, sell_id)?;
    }
    Ok(sell_ids)
}

/// 消费某买入/转换持仓批次的**在用** sell id 列表（issue #940 级联删除的级联对象）。
///
/// 「在用卖出占用」归因谓词：按 `security_lot_sales` 归因到 sell 交易行、只计未软删
/// 者——已删 sell 的历史匹配（幽灵占用）不计入，既不触发守卫、也不阻塞级联查询。
fn active_sell_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT s.sell_transaction_id FROM security_lot_sales s \
         JOIN transactions t ON t.id = s.sell_transaction_id \
         WHERE t.is_deleted = 0 AND s.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 消费某买入/转换持仓批次的**在用** convert id 列表（转换链守卫的对象）。
///
/// 与 [`active_sell_ids_on_own_lots`] 同款归因谓词：按 `security_lot_conversions`
/// 归因到 convert 交易行、只计未软删者。一条转换单拆多腿时，后腿消耗前腿建起的
/// 转入批次，链式依赖由此谓词可见。
fn active_convert_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT c.transaction_id FROM security_lot_conversions c \
         JOIN transactions t ON t.id = c.transaction_id \
         WHERE t.is_deleted = 0 AND c.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 转换链守卫：本行的持仓批次若已被**在用**的后续转换消耗，则拒绝修改/删除。
///
/// 修改会重建批次、删除会连带清空批次，两者都会把后续转换的转出消耗记录一并抹掉
/// （那是它精确回补与结转成本的唯一依据）——链式转换（一条转换单拆多腿）是常态，
/// 所以这条链必须在原始 FIFO 状态还在时从最后一腿往前处理。
///
/// 谓词与 kind 无关（buy 与 convert 同一「在用后续转换消耗」归因），仅按模式选择
/// 用户可见措辞：修改与删除两个入口各自单点（见上方常量）。
fn guard_no_convert_consumed(conn: &Connection, id: &str, mode: Mode) -> Result<()> {
    let (code, msg) = match mode {
        Mode::Update => (
            CONSUMED_BY_CONVERT_CANNOT_UPDATE_CODE,
            CONSUMED_BY_CONVERT_CANNOT_UPDATE,
        ),
        Mode::Delete => (
            CONSUMED_BY_CONVERT_CANNOT_DELETE_CODE,
            CONSUMED_BY_CONVERT_CANNOT_DELETE,
        ),
    };
    if !active_convert_ids_on_own_lots(conn, id)?.is_empty() {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

/// 份额调整重述守卫：本行的持仓批次若已被**在用**的后续份额调整（split）重述，
/// 则拒绝修改/删除（ADR-0106 决策 5 判据家族，先例 [`guard_no_convert_consumed`]）。
///
/// 修改会重建批次、删除会连带清空批次，两者都会把重述审计行一并抹掉——那是
/// split 精确回补与审计的唯一依据。谓词与 kind 无关（buy 与 convert 转入腿同一
/// 「被在用 split 重述」归因），仅按模式选择用户可见措辞。
fn guard_no_split_restated(conn: &Connection, id: &str, mode: Mode) -> Result<()> {
    let (code, msg) = match mode {
        Mode::Update => (
            CONSUMED_BY_SPLIT_CANNOT_UPDATE_CODE,
            CONSUMED_BY_SPLIT_CANNOT_UPDATE,
        ),
        Mode::Delete => (
            CONSUMED_BY_SPLIT_CANNOT_DELETE_CODE,
            CONSUMED_BY_SPLIT_CANNOT_DELETE,
        ),
    };
    if !active_split_ids_on_own_lots(conn, id)?.is_empty() {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

/// 消费某买入/转换持仓批次的**在用** split id 列表（份额调整重述守卫的对象）。
///
/// 与 [`active_sell_ids_on_own_lots`] 同款归因谓词：按 `security_lot_adjustments`
/// 归因到 split 交易行、只计未软删者。
fn active_split_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT a.transaction_id FROM security_lot_adjustments a \
         JOIN transactions t ON t.id = a.transaction_id \
         WHERE t.is_deleted = 0 AND a.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// 份额调整行自身的「批次已被下游在用消耗」守卫（ADR-0106 决策 5 / issue #1051）。
///
/// 归因谓词：本行（split）经 `security_lot_adjustments` 重述过的每个批次，若在
/// **落账序（`transactions.rowid`，全局 FIFO 序）之后**被未软删的后续交易消耗或
/// 再次重述，即拒绝本行改 / 删：
/// - 后续 **sell**（`security_lot_sales` 归因到未软删 sell 行）；
/// - 后续 **convert**（`security_lot_conversions` 归因到未软删 convert 行）；
/// - 后续 **split**（`security_lot_adjustments` 归因到未软删 split 行，排除本行自身）。
///
/// 落账序严格取「之后」而非「任意」：本行重述前的先序 sell / convert 消耗已计入
/// 审计行的 `_before` 快照，回补到 `_before` 与它们相容；只有先序关系之外的消耗才
/// 会与本行的精确回补冲突——这是「下游」二字的可验证判据（判据家族同
/// [`guard_no_split_restated`]，但那条针对买入 / 转换的批次谓词天然不含先序歧义）。
fn guard_no_split_downstream_consumed(conn: &Connection, id: &str, mode: Mode) -> Result<()> {
    let (code, msg) = match mode {
        Mode::Update => (
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE_CODE,
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE,
        ),
        Mode::Delete => (
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE_CODE,
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE,
        ),
    };
    let consumed: bool = conn.query_row(
        "WITH adjusted(lot_id) AS ( \
           SELECT lot_id FROM security_lot_adjustments WHERE transaction_id=?1 \
         ), downstream(txn_id) AS ( \
           SELECT s.sell_transaction_id FROM security_lot_sales s \
            WHERE s.lot_id IN (SELECT lot_id FROM adjusted) \
           UNION ALL \
           SELECT c.transaction_id FROM security_lot_conversions c \
            WHERE c.lot_id IN (SELECT lot_id FROM adjusted) \
           UNION ALL \
           SELECT a.transaction_id FROM security_lot_adjustments a \
            WHERE a.transaction_id<>?1 AND a.lot_id IN (SELECT lot_id FROM adjusted) \
         ) \
         SELECT EXISTS ( \
           SELECT 1 FROM downstream d \
           JOIN transactions t ON t.id = d.txn_id \
           WHERE t.is_deleted = 0 \
             AND t.rowid > (SELECT rowid FROM transactions WHERE id = ?1) \
         )",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if consumed {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

/// 整批清理一笔买入或转换**转入腿**的持仓关联：批次、批次的全部卖出匹配与自身的
/// `security_transactions` 明细行（`transaction_id` 为主键，一交易至多一行——buy 行
/// 或 convert 行之一）。
///
/// 卖出匹配按 `lot_id` 归批清理——指向已软删 sell 的历史匹配行（旧版
/// 「sell 删除不回补」遗留的幽灵占用，issue #940）随批次一并消失；
/// 删除路径（级联后）与修改路径重建（在用占用守卫放行后）共用。
/// 转换行还需先经 [`lots::restore_convert_out_leg`] 回补转出腿（本函数删目标行会按
/// `security_lot_conversions.transaction_id` 的外键级联删掉消耗记录）。
fn purge_lot_artifacts(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM security_lot_sales WHERE lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id=?1)",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_lots WHERE buy_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

/// 在用卖出占用守卫：该行（买入或转换转入腿）的持仓批次已被**在用** sell 的匹配
/// 消耗则拒绝。守卫谓词按在用归因（见 [`active_sell_ids_on_own_lots`]），不再按批次
/// 剩余数量判定：已删 sell 的幽灵扣减不再把行永久锁死（issue #940 修复的死锁根源）。
///
/// `code` / `msg` 为入口单点定义的措辞（本模块常量，ADR-0033 决策 #4）。删除路径不经
/// 本守卫：在用 sell 已被级联消化。
fn guard_no_active_sell(conn: &Connection, id: &str, code: &str, msg: &str) -> Result<()> {
    let partially_sold: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT s.sell_transaction_id) FROM security_lot_sales s \
         JOIN transactions t ON t.id = s.sell_transaction_id \
         WHERE t.is_deleted = 0 AND s.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if partially_sold > 0 {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}
