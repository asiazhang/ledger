//! 份额调整（split）写入测试（ADR-0106 / spec #1045 / issue #1049 + #1050）。
//!
//! 以行为层公开入口断言外部行为（先例：`convert` / `trade` 的测试纪律）：
//! - 按比例重述在用批次：每份成本同比例稀释、**批次总成本在权威口径（锚点 −
//!   记录消耗）下精确不变**、舍入尾差归末批次；逐批次 before / after 落
//!   `security_lot_adjustments` 审计；
//! - **正负两向共用同一公式**（#1050）：`+Δ` 稀释每份成本、`−Δ` 缩股抬升每份
//!   成本，两向批次总成本都精确不变；缩股幅度取严 `|Δ| < 当前持仓`；
//! - **部分卖出按重述后的每份成本结算**（ADR-0106 决策 2 的唯一钉死处：
//!   买 100 @10 元、送 10、卖 55 → 已实现 50 元——不随 FIFO 插入顺序漂移、
//!   不出现 0 成本口径的全额假盈亏）；
//! - split 零已实现盈亏、零账户余额影响（含黑洞）；as-of 时点持仓认 split 腿；
//! - 守卫齐全（码化中文错误）；改 / 删 split 行与进/出 split 的 kind 变更
//!   本票显式拒绝（临时形态，#1051 收编）；批次被在用 split 重述的买入/
//!   转换不可改删（在用占用守卫）；
//! - 历史消耗回算改读记录时成本的**行为保持绑定**：无 split 行时与既有口径恒等。

use crate::transaction::amount::TransactionKind;
use crate::transaction::{
    create_transaction_internal, delete_transaction_internal, update_transaction_internal,
};

use super::super::*;
use super::common::*;
use crate::test_support::{open, seed_account, seed_instrument};

/// 全部未删除账户的**实时**余额快照（`compute_balance` 口径，含黑洞等隐藏账户）。
/// 种子直插的账户无写路径钩子，实时口径才是权威比对基准（ADR-0067）；
/// split 落账前后逐账户比对（同款脚手架见 convert 测试）。
fn balance_snapshot(conn: &rusqlite::Connection) -> Vec<(String, i64)> {
    let mut rows: Vec<(String, i64)> =
        crate::accounts::balance::list_accounts_with_visibility(conn, true)
            .unwrap()
            .into_iter()
            .map(|account| {
                let balance = crate::accounts::balance::compute_balance(conn, &account.id).unwrap();
                (account.id, balance)
            })
            .collect();
    rows.sort();
    rows
}

fn seed_split_scene(conn: &rusqlite::Connection) {
    seed_account(conn, "acc-sp", "股票户", "investment", "CNY", 0);
    seed_instrument(conn, "inst-sp", "502010", "证券基金", "CNY", "unknown");
}

/// 查询该标的全部门批次按 (initial, remaining, cpu) 的列表（按 rowid 序）。
fn lots_of(conn: &rusqlite::Connection, instrument_id: &str) -> Vec<(f64, f64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT initial_quantity, remaining_quantity, cost_per_unit_cents \
             FROM security_lots WHERE instrument_id=?1 ORDER BY rowid",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![instrument_id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

/// 某标的全平仓后的已实现盈亏合计（分）：Σ 卖出匹配 realized_pnl。
fn realized_pnl_of(conn: &rusqlite::Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT COALESCE(SUM(s.realized_pnl_cents),0) FROM security_lot_sales s \
         JOIN security_lots l ON l.id = s.lot_id WHERE l.instrument_id=?1",
        rusqlite::params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 决策 2 的唯一钉死处（ADR-0106）：部分卖出的已实现盈亏按**重述后**的每份成本
/// 结算——买 100 份 @10 元、split +10（每份成本稀释为 10/1.1 ≈ 9.0909 元）、
/// 卖 55 份 @10 元 → 已实现 = 550 − 55 × (1000 ÷ 110) = **50 元**精确。
/// 0 成本新批次方案下同一事实有三个答案（FIFO 排首位 100 元、排末位 0 元），
/// 比例重述给出唯一答案；续卖剩余 55 份全平仓后 Σ已实现 = Σ卖出 − Σ买入 精确闭合。
#[test]
fn split_partial_sell_settles_at_restated_cost() {
    let conn = open();
    seed_split_scene(&conn);
    // 买入 100 份 @10 元：行金额 100000 分（10 万元刻度：100 × 100000 ÷ 100）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    // +10 份份额调整（送股语义）：单批次即末批次，尾差闭合到权威总成本。
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 10.0)).unwrap();

    // 批次重述：数量 110、每份成本 = round(100000 × 100 ÷ 110) = 90909 万分之一元
    //（独立取整与尾差闭合在单批次上重合：闭合目标 = 锚点 100000 分）。
    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 1);
    assert!((lots[0].1 - 110.0).abs() < 1e-9, "剩余数量 × f = 110");
    assert_eq!(
        lots[0].2, 90_909,
        "每份成本 = round(100000 ÷ 1.1)，尾差闭合"
    );

    // 卖 55 份 @10 元：匹配成本 = round(55 × 90909 ÷ 100) = 50000 分（500 元），
    // 已实现 = 55000 − 50000 = 5000 分（50 元）——重述口径的唯一答案。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            55.0,
            100_000,
            "2026-03-01",
        ),
    )
    .unwrap();
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        5_000,
        "部分卖出按重述后每份成本结算：550 − 55×9.0909 = 50 元"
    );

    // 全平仓：耗尽批次闭合 = 锚点 100000 − 记录消耗 50000 = 50000 分，
    // Σ已实现 = 110000 − 100000 = 10000 分精确（成本只在批次间重摊）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            55.0,
            100_000,
            "2026-04-01",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 10_000);
}

/// 多批次尾差归末批次（ADR-0106 决策 2）：非末批次每份成本独立取整稀释，
/// 末批次吸收全部舍入尾差——Σ 权威批次剩余成本（锚点 − 记录消耗）精确不变，
/// 且部分卖出后清仓的闭合恒等式不漂移。
#[test]
fn split_tail_difference_lands_on_last_batch() {
    let conn = open();
    seed_split_scene(&conn);
    // 两批次：100 份 @1.00 元（锚点 10000 分）+ 200 份 @2.00 元（锚点 40000 分）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-01-15",
        ),
    )
    .unwrap();
    // 第一批部分卖出 40 份 @1.00 元：成本 4000 分，锚点剩余 6000 分。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            10_000,
            "2026-01-20",
        ),
    )
    .unwrap();

    // 重述前权威总成本：6000 + 40000 = 46000 分。
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;

    // f = 260/260−...：持仓 260 → 290，f = 290/260。
    // 批次 1（非末）：60 → 60 × 290/260 = 66.923…，cpu = round(10000 ÷ f) =
    // round(8965.517…) = 8966；批次 2（末）：200 → 223.076…，cpu 闭合尾差。
    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 2);
    assert!((lots[0].1 - 60.0 * 290.0 / 260.0).abs() < 1e-9);
    assert_eq!(lots[0].2, 8_966, "非末批次独立取整稀释 round(10000 ÷ f)");
    assert!(
        (lots[1].0 - 200.0 * 290.0 / 260.0).abs() < 1e-9,
        "末批次初始数量同比例放大"
    );

    // 权威总成本精确不变：Σ(锚点 − 记录消耗) = 6000 + 40000 = 46000 分。
    // （末批次 cpu 以该整数为闭合目标倒推，见 split::plan_restatement。）
    let canonical: i64 = conn
        .query_row(
            "SELECT CAST(COALESCE(SUM(t.amount_cents),0) - COALESCE((\
               SELECT SUM(CAST(ROUND(q * cpu / 100.0) AS INTEGER)) FROM (\
                 SELECT s.quantity AS q, s.cost_per_unit_cents AS cpu FROM security_lot_sales s \
                 JOIN security_lots l ON l.id = s.lot_id WHERE l.instrument_id='inst-sp' \
                 UNION ALL \
                 SELECT c.quantity, c.cost_per_unit_cents FROM security_lot_conversions c \
                 JOIN security_lots l ON l.id = c.lot_id WHERE l.instrument_id='inst-sp')),0) AS INTEGER) \
             FROM security_lots l JOIN transactions t ON t.id = l.buy_transaction_id \
             WHERE l.instrument_id='inst-sp' AND l.remaining_quantity > 0",
        [],
        |r| r.get(0),
    )
    .unwrap();
    assert_eq!(canonical, 46_000, "Σ 权威批次剩余成本精确不变");

    // 审计行：逐批次 before / after 快照与批次现状一致。
    let audits: Vec<(f64, f64, f64, f64, i64, i64)> = {
        let mut stmt = conn
            .prepare(
                "SELECT initial_quantity_before, initial_quantity_after, \
                 remaining_quantity_before, remaining_quantity_after, \
                 cost_per_unit_cents_before, cost_per_unit_cents_after \
                 FROM security_lot_adjustments WHERE transaction_id=?1 ORDER BY lot_id",
            )
            .unwrap();
        stmt.query_map(rusqlite::params![split_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
    };
    assert_eq!(audits.len(), 2, "每个在用批次一行审计");
    let mut audits = audits;
    audits.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    assert_eq!(audits[0].2, 60.0, "批次 1 重述前剩余 60");
    assert_eq!(audits[0].4, 10_000, "批次 1 重述前每份成本 10000");
    assert!(
        (audits[1].3 - 200.0 * 290.0 / 260.0).abs() < 1e-9,
        "末批次审计 after 初始数量 = initial × f"
    );
    assert_eq!(
        audits[1].5, lots[1].2,
        "末批次审计 after 每份成本与落库值一致"
    );

    // split 零已实现盈亏：不写卖出匹配。
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        0,
        "split 不产生已实现盈亏"
    );
}

/// 全平仓恒等式不受 split 影响（ADR-0106 决策 7）：买 100 @1.00 → 卖 40 →
/// split +60 → 清仓；Σ已实现 = Σ卖出 − Σ买入 精确成立，不引入新项。
#[test]
fn split_does_not_break_full_liquidation_identity() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    // 卖 40 @2.00 元（8000 分）：成本 4000 分，已实现 4000 分。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            20_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 60.0)).unwrap();
    // 清仓 120 份 @0.50 元（6000 分）：耗尽闭合 = 10000 − 4000 = 6000 分，
    // 已实现 = 6000 − 6000 = 0；Σ已实现 = 4000 = 14000 − 10000 精确。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            120.0,
            5_000,
            "2026-02-20",
        ),
    )
    .unwrap();
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        4_000,
        "Σ已实现 = Σ卖出(14000) − Σ买入(10000)，split 不引入新项"
    );
}

/// 行为保持绑定（ADR-0106 决策 5）：**无 split 行**时，历史消耗回算改读记录时
/// 成本的口径与既有「当前每份成本回算」恒等——含耗尽批次闭合的完整场景逐值
/// 对准手算已知结果（若改口径破坏既有行为，本测试确定性变红）。
#[test]
fn consumption_recalc_without_split_matches_legacy_exact_values() {
    let conn = open();
    seed_split_scene(&conn);
    // 买 100 @1.00 元（锚点 10000 分）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    // 卖 40 @2.00 元：成本 round(40 × 10000 ÷ 100) = 4000 分，已实现 4000 分。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            20_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 4_000);
    // 清仓 60 @0.50 元：耗尽闭合 = 10000 − 4000 = 6000 分（记录时成本口径），
    // 已实现 = 3000 − 6000 = −3000 分；Σ = 1000 = 11000 − 10000。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            60.0,
            5_000,
            "2026-02-20",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 1_000);
}

/// 时点持仓推算认 split 腿（+Δ；ADR-0106 决策 8）：split 落账后 as-of 数量含 Δ，
/// 与 Holding 在「今天」聚合一致（接线负向判据 ADR-0087：删去推算 SQL 的 split
/// 腿，本测试与组合走势测试确定性变红）。
#[test]
fn holdings_as_of_includes_split_leg() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 33.976)).unwrap();

    let today = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-02-01").unwrap();
    assert!(
        (today - 133.976).abs() < 1e-6,
        "as-of 应含 split 腿：100 + 33.976，实际 {today}"
    );
    // 前缀语义：调整日之前不含 Δ。
    let before = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-01-31").unwrap();
    assert!((before - 100.0).abs() < 1e-6);
}

/// 缩股方向（−Δ，ADR-0106 决策 1 / issue #1050）：负增量走**同一重述公式**——
/// 批次数量按比例缩小、每份成本按比例上升、**批次总成本精确不变**；全部账户余额
/// （含黑洞）不变、缩股本身零已实现盈亏；时点持仓推算认带符号 Δ、与持仓视图一致。
#[test]
fn split_reverse_shrinks_holding_and_keeps_total_cost() {
    let conn = open();
    seed_split_scene(&conn);
    // 买 100 @10 元（锚点 100000 分、每份成本 100000 万分之一元）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let balances_before = balance_snapshot(&conn);

    // −20 份缩股：f = 0.8 → 数量 80、每份成本 = 100000 ÷ 0.8 = 125000（12.5 元）。
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -20.0)).unwrap();

    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 1);
    assert!((lots[0].0 - 80.0).abs() < 1e-9, "初始数量 × f = 80");
    assert!((lots[0].1 - 80.0).abs() < 1e-9, "剩余数量 × f = 80");
    assert_eq!(
        lots[0].2, 125_000,
        "每份成本 = 100000 ÷ 0.8，批次总成本精确不变"
    );

    // 持仓减少 |Δ|；持仓视图（批次驱动）总成本不变：80 × 125000 ÷ 100 = 100000 分。
    let (qty, cost_basis): (f64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings \
             WHERE account_id='acc-sp' AND instrument_id='inst-sp'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((qty - 80.0).abs() < 1e-6, "持仓 100 − 20 = 80，实际 {qty}");
    assert_eq!(cost_basis, 100_000, "批次总成本不变");

    // 全部账户余额（含黑洞）不变（无现金腿）、缩股零已实现盈亏。
    assert_eq!(
        balance_snapshot(&conn),
        balances_before,
        "缩股无现金腿，余额不变"
    );
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        0,
        "缩股本身不产生已实现盈亏"
    );

    // 时点持仓推算认带符号 Δ：调整日当天含 −20、调整日前不含。
    let on = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-02-01").unwrap();
    assert!(
        (on - 80.0).abs() < 1e-6,
        "as-of 应含缩股腿：100 − 20 = 80，实际 {on}"
    );
    let before = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-01-31").unwrap();
    assert!(
        (before - 100.0).abs() < 1e-6,
        "调整日之前不含 Δ，实际 {before}"
    );
}

/// 缩股方向的多批次尾差归末批次（ADR-0106 决策 2 在负方向的独立钉死）：两批次
/// 缩股时非末批次每份成本独立取整上升，末批次以其为闭合目标吸收全部舍入尾差——
/// Σ 权威批次剩余成本（锚点 − 记录消耗）精确不变；同一点上时点持仓推算、批次
/// 剩余合计与持仓视图三口径一致。
#[test]
fn split_reverse_tail_difference_lands_on_last_batch() {
    let conn = open();
    seed_split_scene(&conn);
    // 两批次：100 份 @1.00 元（锚点 10000 分）+ 200 份 @2.00 元（锚点 40000 分）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-01-15",
        ),
    )
    .unwrap();

    // 缩股 −30：持仓 300 → 270，f = 0.9。
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -30.0)).unwrap();

    // 批次 1（非末）：100 → 90，cpu = round(10000 ÷ 0.9) = round(11111.111) = 11111；
    // 批次 2（末）：200 → 180，cpu 倒推闭合全部尾差。
    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots.len(), 2);
    assert!((lots[0].1 - 90.0).abs() < 1e-9, "非末批次剩余 = 100 × 0.9");
    assert_eq!(lots[0].2, 11_111, "非末批次独立取整上升 round(10000 ÷ f)");
    assert!((lots[1].1 - 180.0).abs() < 1e-9, "末批次剩余 = 200 × 0.9");
    assert_eq!(
        lots[1].2, 22_222,
        "末批次闭合尾差：round((50000 − 90×11111÷100) × 100 ÷ 180)"
    );

    // Σ 权威批次剩余成本精确不变：锚点合计 50000 分（重述不改它）。
    let canonical: i64 = conn
        .query_row(
            "SELECT CAST(COALESCE(SUM(t.amount_cents),0) - COALESCE((\
               SELECT SUM(CAST(ROUND(q * cpu / 100.0) AS INTEGER)) FROM (\
                 SELECT s.quantity AS q, s.cost_per_unit_cents AS cpu FROM security_lot_sales s \
                 JOIN security_lots l ON l.id = s.lot_id WHERE l.instrument_id='inst-sp' \
                 UNION ALL \
                 SELECT c.quantity, c.cost_per_unit_cents FROM security_lot_conversions c \
                 JOIN security_lots l ON l.id = c.lot_id WHERE l.instrument_id='inst-sp')),0) AS INTEGER) \
             FROM security_lots l JOIN transactions t ON t.id = l.buy_transaction_id \
             WHERE l.instrument_id='inst-sp' AND l.remaining_quantity > 0",
        [],
        |r| r.get(0),
    )
    .unwrap();
    assert_eq!(canonical, 50_000, "Σ 权威批次剩余成本精确不变");

    // 三口径一致：as-of 时点持仓推算 = 批次剩余合计 = 持仓视图数量。
    let lot_sum: f64 = lots.iter().map(|l| l.1).sum();
    let as_of = holdings::holdings_as_of(&conn, Some("inst-sp"), "2026-02-01").unwrap();
    let view_qty: f64 = conn
        .query_row(
            "SELECT quantity FROM v_holdings WHERE account_id='acc-sp' AND instrument_id='inst-sp'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((lot_sum - 270.0).abs() < 1e-9);
    assert!(
        (as_of - lot_sum).abs() < 1e-6,
        "as-of {as_of} 应对齐批次合计 {lot_sum}"
    );
    assert!(
        (view_qty - lot_sum).abs() < 1e-6,
        "持仓视图 {view_qty} 应对齐批次合计 {lot_sum}"
    );

    // 缩股零已实现盈亏：不写卖出匹配。
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 0);
}

/// 缩股后部分卖出按**重述后**每份成本结算（ADR-0106 决策 2 在负方向的同一钉死）：
/// 买 100 @10 元、缩股 −20（每份成本升为 12.5 元）、卖 60 @15 元 →
/// 已实现 = 900 − 750 = 150 元；续卖剩余 20 清仓后 Σ已实现 = Σ卖出 − Σ买入 精确闭合。
#[test]
fn split_reverse_partial_sell_settles_at_restated_cost() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -20.0)).unwrap();

    // 卖 60 @15 元：金额 90000 分，成本 round(60 × 125000 ÷ 100) = 75000 分，
    // 已实现 15000 分——按重述后每份成本 12.5 元结算的唯一答案。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            60.0,
            150_000,
            "2026-03-01",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 15_000);

    // 清仓剩余 20 @15 元：耗尽闭合 = 100000 − 75000 = 25000 分，已实现 5000 分；
    // Σ = 20000 = Σ卖出 120000 − Σ买入 100000（缩股只重摊成本，不引入新项）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            20.0,
            150_000,
            "2026-04-01",
        ),
    )
    .unwrap();
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 20_000);
}

/// 守卫（ADR-0106 决策 7，全部码化中文错误）：零持仓、Δ = 0、缩股越界
/// （`|Δ| ≥ 当前持仓`，取严）、非投资账户、无现金腿（非零金额 / 单价 / 手续费）、
/// 携带转入标的 / 转入账户 / 出资账户 / 商户 / 分类 / 保单。
#[test]
fn split_guards_return_coded_errors() {
    let conn = open();
    seed_split_scene(&conn);
    seed_account(&conn, "acc-cash", "现金户", "cash", "CNY", 0);
    seed_instrument(&conn, "inst-other", "600519", "贵州茅台", "CNY", "sh");

    let err =
        |r: crate::error::Result<crate::transaction::TransactionWrite>| r.expect_err("应被拒绝");

    // 零持仓拒绝（Δ>0 亦须有在用持仓——价值已含在原批次，零持仓无从重述）。
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-other", 10.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-no-holding", "{e:?}");
    // 零持仓缩股：`|Δ| > 0 ≥ 当前持仓` 被缩股守卫拒绝（对准缩股语义，不借用
    // 卖出 / 超卖文案），不给「零持仓无从重述」的正向口径。
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-other", -10.0),
    ));
    assert_eq!(
        e.code().unwrap(),
        "trade.split-shrink-not-less-than-holding",
        "{e:?}"
    );

    // 建仓后逐守卫。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-sp", 0.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-quantity-zero");
    // 缩股取严（ADR-0106 决策 1/7）：等号（Δ = −100 = −持仓）拒绝——f = 0 会让
    // 成本凭空消失；越界同样拒绝。文案对准缩股语义（非卖出 / 超卖）。
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-sp", -100.0),
    ));
    assert_eq!(
        e.code().unwrap(),
        "trade.split-shrink-not-less-than-holding",
        "{e:?}"
    );
    assert!(e.to_string().contains("缩股"), "文案应针对缩股语义: {e}");
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-sp", -150.0),
    ));
    assert_eq!(
        e.code().unwrap(),
        "trade.split-shrink-not-less-than-holding",
        "{e:?}"
    );

    // 非投资账户。
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-cash", "inst-sp", 10.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-account-not-investment");

    // 无现金腿：非零金额 / 单价 / 非零手续费。
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.amount_cents = 100;
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-amount-forbidden");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.price_cents = Some(10_000);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-price-forbidden");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.fee_cents = Some(1);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-fee-forbidden");

    // 单标的、不跨账户。
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.to_instrument_id = Some("inst-other".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-to-instrument-forbidden");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.to_account_id = Some("acc-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.split-to-account-forbidden");

    // 出资账户（split 不在出资闭集内）。
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.funding_account_id = Some("acc-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.funding-unsupported");

    // 商户 / 分类 / 保单（参考数据携带准入：split 不在任何准入集）。
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.merchant_name = Some("京东".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.merchant-unsupported");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.category_id = Some("cat-x".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.category-unsupported");
    let mut input = make_split_input("acc-sp", "inst-sp", 10.0);
    input.policy_id = Some("pol-x".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.policy-unsupported");

    // 标的不存在。
    let e = err(create_transaction_internal(
        &conn,
        make_split_input("acc-sp", "inst-missing", 10.0),
    ));
    assert_eq!(e.code().unwrap(), "trade.split-instrument-not-found");

    // 全部拒绝不落库：split 行数为 0、批次未被扰动。
    let splits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE action='split'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(splits, 0, "被拒的份额调整不应落库");
    assert!((lots_of(&conn, "inst-sp")[0].1 - 100.0).abs() < 1e-9);
}

/// 进/出 split 的 kind 变更一律拒绝（ADR-0106 决策 5，与 convert 同规，永久守卫）：
/// 就地修改与删除已随 #1051 收编，不再是「暂不支持」，但换 kind 仍只有「软删 + 重建」
/// 一条路。
#[test]
fn split_kind_change_is_rejected() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0))
        .unwrap()
        .id;

    // 改出 split（→ buy）与改入 split（buy → split）都拒绝。
    let e = update_transaction_internal(
        &conn,
        &split_id,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            10.0,
            10_000,
            "2026-02-01",
        ),
    )
    .expect_err("改出 split 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-kind-change-forbidden");
    let buy_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            10.0,
            10_000,
            "2026-01-20",
        ),
    )
    .unwrap()
    .id;
    let e = update_transaction_internal(&conn, &buy_id, make_split_input("acc-sp", "inst-sp", 5.0))
        .expect_err("改入 split 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-kind-change-forbidden");
}

/// 某 split 的重述审计行（remaining before / after、cpu before / after，按批次
/// rowid 序）——回补精确性与「审计随新输入重建」的断言依据。
fn audit_rows(conn: &rusqlite::Connection, split_id: &str) -> Vec<(f64, f64, i64, i64)> {
    let mut stmt = conn
        .prepare(
            "SELECT a.remaining_quantity_before, a.remaining_quantity_after, \
             a.cost_per_unit_cents_before, a.cost_per_unit_cents_after \
             FROM security_lot_adjustments a JOIN security_lots l ON l.id = a.lot_id \
             WHERE a.transaction_id=?1 ORDER BY l.rowid",
        )
        .unwrap();
    stmt.query_map(rusqlite::params![split_id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

/// split 扩展行数量（读回 qty 与行金额，验证「全字段替换」真换）。
fn split_extension(conn: &rusqlite::Connection, split_id: &str) -> (f64, i64) {
    conn.query_row(
        "SELECT st.quantity, t.amount_cents FROM security_transactions st \
         JOIN transactions t ON t.id = st.transaction_id WHERE st.transaction_id=?1",
        rusqlite::params![split_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .unwrap()
}

/// 就地修改一笔份额调整（#1051 / ADR-0106 决策 3/5）：先按 `security_lot_adjustments`
/// 逐批次 before 快照精确回退（含舍入），再按新输入重述——结果与「直接以新输入落一笔
/// 调整」逐值一致，审计随新快照重建、旧快照不残留；余额不变、零已实现盈亏。
#[test]
fn split_update_reallocates_lots_to_new_input() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0))
        .unwrap()
        .id;
    // +50 后：150 份、每份成本 round(100000 ÷ 1.5) = 66667 万分之一元。
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(150.0, 150.0, 66_667)]);

    let before = balance_snapshot(&conn);
    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 100.0),
    )
    .unwrap();

    // 回退到 100 份 @100000，再按 +100 重述：200 份、每份成本 50000。
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 200.0, 50_000)]);
    // 审计随新输入重建：仍是 1 行，before 仍为调整前 100、after 为新值 200。
    assert_eq!(
        audit_rows(&conn, &split_id),
        vec![(100.0, 200.0, 100_000, 50_000)]
    );
    // 全字段替换：扩展行数量随新 Δ，行金额仍恒 0（无现金腿）。
    assert_eq!(split_extension(&conn, &split_id), (100.0, 0));
    assert_eq!(before, balance_snapshot(&conn), "修改前后余额不变");
    assert_eq!(realized_pnl_of(&conn, "inst-sp"), 0, "split 恒零已实现盈亏");
}

/// 修改可跨正负两向（#1051）：增长改缩股、缩股改增长都走同一回退 + 重述路径，
/// 批次数量与每份成本精确落到新输入对应的值。
#[test]
fn split_update_crosses_growth_and_shrink_direction() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 200.0, 50_000)]);

    // 增长 → 缩股：回退到 100，再按 −40 重述为 60 份、每份成本 round(100000 ÷ 0.6)。
    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", -40.0),
    )
    .unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(60.0, 60.0, 166_667)]);

    // 缩股 → 增长：回退到 100，再按 +300 重述为 400 份、每份成本 25000。
    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 300.0),
    )
    .unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(400.0, 400.0, 25_000)]);
}

/// 修改不会回头改后续买入的批次（#1051 / ADR-0106 决策 6）：重述目标以本行**落账序**
/// 为界——其后才建立的批次（后续买入）不受这笔历史份额调整的修改 / 删除影响。
#[test]
fn split_update_leaves_later_buys_untouched() {
    let conn = open();
    seed_split_scene(&conn);
    // 批次 A：100 份 @1.00 元（锚点 10000 分）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0))
        .unwrap()
        .id;
    // 批次 B：份额调整**之后**才买入 200 份 @2.00 元（锚点 40000 分）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-03-01",
        ),
    )
    .unwrap();
    let later_buy = lots_of(&conn, "inst-sp")[1];

    // 改 split：+50 → +100。A 回退到 100 后按 f=2 重述为 200 份 @5000。
    update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 100.0),
    )
    .unwrap();
    let lots = lots_of(&conn, "inst-sp");
    assert_eq!(lots[0], (200.0, 200.0, 5_000));
    assert_eq!(
        lots[1], later_buy,
        "后续买入的批次不因历史 split 的修改而重述"
    );
    assert_eq!(
        audit_rows(&conn, &split_id).len(),
        1,
        "审计只覆盖在场批次 A"
    );

    // 删除也只精确还原 A，后建的 B 原样保留。
    delete_transaction_internal(&conn, &split_id).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(100.0, 100.0, 10_000), later_buy]
    );
}

/// 删除一笔份额调整（#1051 / ADR-0106 决策 3）：按逐批次 before 快照精确还原
/// 持仓与每份成本（含舍入），审计行与 split 扩展行一并消失；先序卖出的已实现盈亏
/// 不受影响、全部账户余额不变。
#[test]
fn split_delete_restores_lots_exactly_with_rounding() {
    let conn = open();
    seed_split_scene(&conn);
    // 两批次 + 先序部分卖出，制造非末批次独立取整的舍入尾差（同尾差测试场景）。
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-01",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            200.0,
            20_000,
            "2026-01-15",
        ),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            20_000,
            "2026-01-20",
        ),
    )
    .unwrap();
    let lots_before_split = lots_of(&conn, "inst-sp");
    let pnl_before_split = realized_pnl_of(&conn, "inst-sp");
    let balances_before_split = balance_snapshot(&conn);

    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 30.0))
        .unwrap()
        .id;
    assert_ne!(
        lots_of(&conn, "inst-sp"),
        lots_before_split,
        "重述应改变批次"
    );
    assert_eq!(
        audit_rows(&conn, &split_id).len(),
        2,
        "每个在用批次一行审计"
    );

    delete_transaction_internal(&conn, &split_id).unwrap();

    assert_eq!(
        lots_of(&conn, "inst-sp"),
        lots_before_split,
        "删除后批次逐值精确还原到调整前（含舍入）"
    );
    assert!(audit_rows(&conn, &split_id).is_empty(), "审计随删除消失");
    let ext: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_transactions WHERE transaction_id=?1",
            rusqlite::params![split_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ext, 0, "split 扩展行随删除消失");
    let deleted: i64 = conn
        .query_row(
            "SELECT is_deleted FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1, "交易行软删");
    assert_eq!(
        realized_pnl_of(&conn, "inst-sp"),
        pnl_before_split,
        "先序卖出的已实现盈亏不受删除影响"
    );
    assert_eq!(
        balance_snapshot(&conn),
        balances_before_split,
        "删除前后余额不变"
    );
}

/// 下游在用消耗守卫（#1051 / ADR-0106 决策 5）：本行重述过的批次被**落账序在后、
/// 且未软删**的后续 sell 消耗时，改 / 删一律码化拒绝（回补会把批次还原到重述前快照，
/// 与下游按重述后口径的结算冲突）；下游 sell 被软删消化后守卫解除，删除精确还原。
#[test]
fn split_downstream_consumption_guards_update_and_delete() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    // 下游卖出 50 份：消耗本行重述过的批次（200 → 150）。
    let sell_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            50.0,
            100_000,
            "2026-03-01",
        ),
    )
    .unwrap()
    .id;
    let lots_after_sell = lots_of(&conn, "inst-sp");

    let e = update_transaction_internal(
        &conn,
        &split_id,
        make_split_input("acc-sp", "inst-sp", 60.0),
    )
    .expect_err("被下游消耗的份额调整修改应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-update", "{e:?}");
    let e = delete_transaction_internal(&conn, &split_id)
        .expect_err("被下游消耗的份额调整删除应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-delete", "{e:?}");
    // 拒绝无副作用：批次、审计与扩展行原样。
    assert_eq!(lots_of(&conn, "inst-sp"), lots_after_sell);
    assert_eq!(audit_rows(&conn, &split_id).len(), 1);
    assert_eq!(split_extension(&conn, &split_id), (100.0, 0));

    // 下游 sell 软删（回补持仓）后守卫解除：删除 split 精确还原到调整前 100 @100000。
    delete_transaction_internal(&conn, &sell_id).unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 200.0, 50_000)]);
    delete_transaction_internal(&conn, &split_id).unwrap();
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(100.0, 100.0, 100_000)]);
}

/// 下游在用 convert 消耗本行重述过的批次同样挂守卫（#1051）——转出腿 FIFO 消耗
/// 记在 `security_lot_conversions`，谓词与 sell 同族。
#[test]
fn split_downstream_convert_guards_delete() {
    let conn = open();
    seed_split_scene(&conn);
    seed_instrument(&conn, "inst-other", "600519", "贵州茅台", "CNY", "sh");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    // 下游转换 80 份 inst-sp → inst-other：消耗本行重述过的批次。
    create_transaction_internal(
        &conn,
        make_convert_input("acc-sp", "inst-sp", "inst-other", 80.0, 80.0, 800, 800, 0),
    )
    .unwrap();

    let e = delete_transaction_internal(&conn, &split_id)
        .expect_err("被下游转换消耗的份额调整删除应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-delete", "{e:?}");
}

/// 后一次 split 重述了本行重述过的批次同样挂守卫（#1051）：删除前一行会破坏后一行
/// 的 before 链；后一行无下游可删，删后再删前一行精确链式还原。
#[test]
fn split_later_split_guards_earlier_split_delete() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let first = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    let second = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 100.0))
        .unwrap()
        .id;
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(300.0, 300.0, 33_333)]);

    let e = delete_transaction_internal(&conn, &first).expect_err("前一行被后一行重述应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.split-consumed-delete", "{e:?}");

    delete_transaction_internal(&conn, &second).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(200.0, 200.0, 50_000)],
        "删后一行先还原到它重述前（= 前一行重述后）"
    );
    delete_transaction_internal(&conn, &first).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(100.0, 100.0, 100_000)],
        "链条自后向前拆除后精确还原到调整前"
    );
}

/// 先序消耗不挂守卫（#1051 /「下游」的可验证判据）：落账序在 split **之前**的 sell
/// 已计入重述前快照，回补到 before 与它相容——删除 split 不应被误拒，先序卖出的
/// 匹配记录完整保留。
#[test]
fn split_upstream_consumption_does_not_block_delete() {
    let conn = open();
    seed_split_scene(&conn);
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            100_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    let sell_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Sell,
            "acc-sp",
            "inst-sp",
            40.0,
            100_000,
            "2026-01-20",
        ),
    )
    .unwrap()
    .id;
    let split_id = create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 60.0))
        .unwrap()
        .id;
    // 先序卖出 40 份后剩 60 @100000；split +60（f = 120/60 = 2）→
    // 初始数量 100×2 = 200、剩余 120、每份成本 50000。
    assert_eq!(lots_of(&conn, "inst-sp"), vec![(200.0, 120.0, 50_000)]);

    delete_transaction_internal(&conn, &split_id).unwrap();
    assert_eq!(
        lots_of(&conn, "inst-sp"),
        vec![(100.0, 60.0, 100_000)],
        "回补到 split 前（先序卖出后的）批次状态"
    );
    let sell_matches: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lot_sales WHERE sell_transaction_id=?1",
            rusqlite::params![sell_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(sell_matches, 1, "先序卖出的匹配记录完整保留");
}

/// 在用占用守卫（ADR-0106 决策 5 判据家族，先例「被后续转换消耗」）：批次已被
/// **在用** split 重述的买入不可改、不可删——删除会连批次与重述审计一并抹掉，
/// split 行失去精确回补与审计的唯一依据。
#[test]
fn buy_restated_by_active_split_cannot_update_or_delete() {
    let conn = open();
    seed_split_scene(&conn);
    let buy_id = create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap()
    .id;
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0)).unwrap();

    let e = update_transaction_internal(
        &conn,
        &buy_id,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            110.0,
            10_000,
            "2026-01-10",
        ),
    )
    .expect_err("被重述批次的买入修改应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.consumed-by-split-update");
    let e = delete_transaction_internal(&conn, &buy_id).expect_err("被重述批次的买入删除应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.consumed-by-split-delete");
}

/// split 落账前后全部账户余额（含黑洞）完全不变、交易行无现金腿（弱表述的
/// 域侧对拍；HTTP 侧端到端见 api_server::batch_import）。
#[test]
fn split_keeps_all_account_balances_unchanged() {
    let conn = open();
    seed_split_scene(&conn);
    seed_account(&conn, "acc-hole", "黑洞", "other", "CNY", 0);
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hole'", [])
        .unwrap();
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();

    let before = balance_snapshot(&conn);
    let split_id =
        create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 33.976))
            .unwrap()
            .id;
    let after = balance_snapshot(&conn);
    assert_eq!(
        before, after,
        "split 落账前后全部账户余额（含黑洞）完全不变"
    );

    // 交易行无现金腿：金额与本位币恒 0、币种随投资账户。
    let (amount, native, currency): (i64, i64, String) = conn
        .query_row(
            "SELECT amount_cents, amount_native_cents, currency_code FROM transactions WHERE id=?1",
            rusqlite::params![split_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((amount, native), (0, 0));
    assert_eq!(currency, "CNY");
}

/// 组合走势认 split 腿（ADR-0106 决策 8；接线负向判据 ADR-0087：删去 as-of 推算
/// 的 split 腿，本测试确定性变红）——split 落账后的周点市值 = 含 Δ 的当期持仓 ×
/// 当期周线价格，走势查询不感知推算模块内部变化。
#[test]
fn portfolio_trend_includes_split_leg() {
    let conn = open();
    seed_split_scene(&conn);
    crate::test_support::seed_price_history(&conn, "ph-sp", "inst-sp", "2026-02-02", 10_000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", 50.0)).unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.points.len(), 1);
    assert_eq!(
        trend.points[0].market_value_cents, 15_000,
        "周点市值 = (100 + 50) × 1.00 元 = 15000 分"
    );
}

/// 组合走势认缩股腿（−Δ，ADR-0106 决策 8 / issue #1050）：缩股落账后的周点市值 =
/// 缩股后持仓 × 当期周线价格——与 as-of 推算、持仓视图同源，走势查询不感知推算
/// 内部变化；删去推算 SQL 的 split 腿本测试确定性变红（接线负向判据 ADR-0087）。
#[test]
fn portfolio_trend_includes_reverse_split_leg() {
    let conn = open();
    seed_split_scene(&conn);
    crate::test_support::seed_price_history(&conn, "ph-rs", "inst-sp", "2026-02-02", 10_000, "CNY");
    create_transaction_internal(
        &conn,
        make_trade_input(
            TransactionKind::Buy,
            "acc-sp",
            "inst-sp",
            100.0,
            10_000,
            "2026-01-10",
        ),
    )
    .unwrap();
    create_transaction_internal(&conn, make_split_input("acc-sp", "inst-sp", -40.0)).unwrap();

    let trend = trend::query_portfolio_value_trend(&conn, &TrendRange::default()).unwrap();
    assert_eq!(trend.points.len(), 1);
    assert_eq!(
        trend.points[0].market_value_cents, 6_000,
        "周点市值 = (100 − 40) × 1.00 元 = 6000 分"
    );
}
