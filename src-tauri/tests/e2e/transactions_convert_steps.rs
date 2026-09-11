//! 基金转换（convert）跨模块用户旅程 BDD 步骤（issue #982 / ADR-0099）：
//! 以存量样例两腿转换（2020-05-11，006793 → 519700 / 519772）为锚，在旅程层钉住
//! spec 验收 1/2/3——落账前后全部账户余额（含黑洞）不变、持仓换手、结转成本逐腿
//! 占比（含分舍入闭合）、全平仓 Σ 已实现盈亏不受转换扰动——以及生命周期（编辑
//! 回退精确复原、删除级联后重导）与回归扫尾。
//!
//! 写入一律经 L1 [`crate::step_inputs::convert_input`] 工厂 + L2 行为层动词
//! （[`crate::step_verbs`]），断言读扩展表投影与消耗/匹配记录（与 IPC 读命令
//! 同一数据源）；余额快照消费账户域 `list_account_balances_for_api`（含隐藏/黑洞）。

use std::collections::HashMap;

use cucumber::{then, when};
use rusqlite::params;

use tauri_app_lib::accounts::list_account_balances_for_api;
use tauri_app_lib::transaction::TransactionInput;

use crate::common::{capture_expected_error, instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::convert_input;
use crate::step_verbs::{
    create_transaction_verb, delete_transaction_verb, try_delete_transaction_verb,
    update_transaction_verb,
};
use crate::world::LedgerWorld;

/// 按转入标的定位转换交易 id（两腿场景内同转入标的唯一；不存在即场景文本错误）。
fn convert_txn_id(world: &LedgerWorld, to_symbol: &str) -> String {
    world_conn!(world)
        .query_row(
            "SELECT st.transaction_id FROM security_transactions st \
             JOIN instruments i ON i.id = st.to_instrument_id \
             WHERE i.symbol=?1 AND st.action='convert'",
            params![to_symbol],
            |r| r.get(0),
        )
        .expect("未找到对应转换交易")
}

/// 转换交易的扩展表投影行（两腿明细 + 手续费）。
struct ConvertDetail {
    out_instrument_id: String,
    quantity: f64,
    to_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    fee_cents: i64,
}

fn convert_detail(world: &LedgerWorld, to_symbol: &str) -> ConvertDetail {
    let id = convert_txn_id(world, to_symbol);
    world_conn!(world)
        .query_row(
            "SELECT st.instrument_id, st.quantity, st.to_quantity, st.out_amount_cents, \
                    st.in_amount_cents, st.fee_cents \
             FROM security_transactions st WHERE st.transaction_id=?1 AND st.action='convert'",
            params![id],
            |r| {
                Ok(ConvertDetail {
                    out_instrument_id: r.get(0)?,
                    quantity: r.get(1)?,
                    to_quantity: r.get(2)?,
                    out_amount_cents: r.get(3)?,
                    in_amount_cents: r.get(4)?,
                    fee_cents: r.get(5)?,
                })
            },
        )
        .expect("读取转换明细失败")
}

// ---------------------------------------------------------------------------
// When：转换写入（创建 / 修改 / 删除）
// ---------------------------------------------------------------------------

/// 按确认单录入一笔基金转换（issue #982 / ADR-0099 单腿一条记录）：两腿份额与
/// 两侧确认金额为权威输入，行金额锚点由服务端按 FIFO 结转成本重算。
#[when(
    expr = "按确认单于 {string} 转换 {string} 份额 {float} 为 {string} 份额 {float} 转出金额 {int} 转入金额 {int} 手续费 {int} 到投资账户 {string}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn convert_fund(
    world: &mut LedgerWorld,
    date: String,
    out_symbol: String,
    out_quantity: f64,
    in_symbol: String,
    in_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    fee_cents: i64,
    account_name: String,
) {
    let conn = world_conn!(world);
    let out_instrument_id = instrument_id_by_symbol(&conn, &out_symbol);
    let in_instrument_id = instrument_id_by_symbol(&conn, &in_symbol);
    drop(conn);
    let account_id = world.account_id(&account_name);
    let input = TransactionInput {
        fee_cents: Some(fee_cents),
        ..convert_input(
            &account_id,
            &out_instrument_id,
            out_quantity,
            &in_instrument_id,
            in_quantity,
            out_amount_cents,
            in_amount_cents,
            &date,
        )
    };
    create_transaction_verb(world, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 修改一笔转换（全字段替换，issue #979）：转出/转入标的沿用原行（场景以转入标的
/// 定位），两腿份额、两侧确认金额与手续费为编辑入参；行金额占位 0（服务端按新
/// 输入重算结转成本，与创建同款）。
#[when(
    expr = "修改转换（转入 {string}）为 转出份额 {float} 转入份额 {float} 转出金额 {int} 转入金额 {int} 手续费 {int}"
)]
fn update_convert(
    world: &mut LedgerWorld,
    to_symbol: String,
    out_quantity: f64,
    in_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    fee_cents: i64,
) {
    let id = convert_txn_id(world, &to_symbol);
    let (out_instrument_id, to_instrument_id): (String, String) = world_conn!(world)
        .query_row(
            "SELECT instrument_id, to_instrument_id FROM security_transactions \
             WHERE transaction_id=?1 AND action='convert'",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("读取转换原行失败");
    let existing = world
        .txn
        .transactions_list
        .iter()
        .find(|t| t.id == id)
        .expect("原转换交易不存在")
        .clone();
    let input = TransactionInput {
        amount_cents: 0,
        instrument_id: Some(out_instrument_id),
        to_instrument_id: Some(to_instrument_id),
        quantity: Some(out_quantity),
        to_quantity: Some(in_quantity),
        out_amount_cents: Some(out_amount_cents),
        in_amount_cents: Some(in_amount_cents),
        fee_cents: Some(fee_cents),
        ..crate::step_inputs::existing_input(&existing)
    };
    update_transaction_verb(world, &id, input);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 删除一笔转换（issue #979 删除级联）：转入批次的在用 sell 逐笔软删、转出腿
/// 精确回补、转入批次与消耗记录整批清理。
#[when(expr = "删除转换（转入 {string}）")]
fn delete_convert(world: &mut LedgerWorld, to_symbol: String) {
    let id = convert_txn_id(world, &to_symbol);
    delete_transaction_verb(world, &id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

/// 尝试删除一笔买入并捕获错误（issue #982 回归扫尾）：被转换消耗过的买入删除
/// 被守卫拒绝（`trade.consumed-by-convert-delete`），纠错只有「先处理下游转换」
/// 一条窄路；既有级联语义不受转换引入而松动。
#[when(expr = "尝试删除买入交易 {string}")]
fn try_delete_buy(world: &mut LedgerWorld, symbol: String) {
    let id = crate::transactions_edit_steps::trade_txn_id(world, &symbol, "buy");
    let result = try_delete_transaction_verb(world, &id);
    capture_expected_error(world, result.map(|_| ()));
}

// ---------------------------------------------------------------------------
// Then：旅程断言
// ---------------------------------------------------------------------------

/// 验收 1（issue #982）：转换落账前后全部账户余额（含黑洞）完全不变——重算
/// 实时余额逐账户比对「查询全部账户余额」步骤留下的快照；黑洞（隐藏）账户
/// 必须在快照内，防止比较集合静默缩水。
#[then(expr = "全部账户余额应与快照一致（含黑洞）")]
fn assert_balances_unchanged(world: &mut LedgerWorld) {
    let balances = list_account_balances_for_api(&world_conn!(world)).expect("查询账户余额失败");
    let current: HashMap<String, (i64, bool)> = balances
        .into_iter()
        .map(|ab| (ab.account.name, (ab.balance_cents, ab.account.is_hidden)))
        .collect();
    assert!(
        !world.txn.balances.is_empty(),
        "余额快照为空：先走「查询全部账户余额」"
    );
    assert!(
        world.txn.balances.values().any(|(_, hidden)| *hidden),
        "余额快照应包含黑洞（隐藏）账户，实际 {:?}",
        world.txn.balances.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        current, world.txn.balances,
        "转换落账前后全部账户余额（含黑洞）应完全不变"
    );
}

/// 验收 3（issue #982）：单腿结转成本（行金额锚点，分）——转出批次原始成本
/// 按 FIFO 结转，不按转入日市值重置。
#[then(expr = "转换转入 {string} 的结转成本应为 {int}")]
fn assert_convert_carried_cost(world: &mut LedgerWorld, to_symbol: String, expected: i64) {
    let id = convert_txn_id(world, &to_symbol);
    let carried: i64 = world_conn!(world)
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .expect("读取转换行金额锚点失败");
    assert_eq!(carried, expected, "转换转入 {to_symbol} 的结转成本不符");
}

/// 验收 3（issue #982）：全部转换结转成本合计（分）——多腿占比分摊后逐腿相加
/// 精确闭合到合计（尾差末腿吸收）。
#[then(expr = "全部转换结转成本合计应为 {int}")]
fn assert_total_carried_cost(world: &mut LedgerWorld, expected: i64) {
    let total: i64 = world_conn!(world)
        .query_row(
            "SELECT COALESCE(SUM(amount_cents),0) FROM transactions \
             WHERE kind='convert' AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .expect("读取结转成本合计失败");
    assert_eq!(total, expected, "全部转换结转成本合计不符");
}

/// 转换手续费如实记录在转换行上（不摊入结转成本、不产生现金腿）。
#[then(expr = "转换转入 {string} 的手续费应为 {int}")]
fn assert_convert_fee(world: &mut LedgerWorld, to_symbol: String, expected: i64) {
    let detail = convert_detail(world, &to_symbol);
    assert_eq!(
        detail.fee_cents, expected,
        "转换转入 {to_symbol} 的手续费不符"
    );
}

/// 两腿明细断言（issue #982）：转出标的、两侧份额与确认金额逐项核对平台确认单。
#[then(
    expr = "转换转入 {string} 的明细应为 转出 {string} 份额 {float} 转入份额 {float} 转出金额 {int} 转入金额 {int} 手续费 {int}"
)]
#[allow(clippy::too_many_arguments)] // cucumber step 签名由表达式参数决定，无法缩减
fn assert_convert_detail(
    world: &mut LedgerWorld,
    to_symbol: String,
    out_symbol: String,
    out_quantity: f64,
    in_quantity: f64,
    out_amount_cents: i64,
    in_amount_cents: i64,
    fee_cents: i64,
) {
    let expected_out = instrument_id_by_symbol(&world_conn!(world), &out_symbol);
    let detail = convert_detail(world, &to_symbol);
    assert_eq!(detail.out_instrument_id, expected_out, "转出标的不符");
    assert!(
        (detail.quantity - out_quantity).abs() < 1e-9,
        "转出份额不符: 期望 {out_quantity}，实际 {}",
        detail.quantity
    );
    assert!(
        (detail.to_quantity - in_quantity).abs() < 1e-9,
        "转入份额不符: 期望 {in_quantity}，实际 {}",
        detail.to_quantity
    );
    assert_eq!(detail.out_amount_cents, out_amount_cents, "转出金额不符");
    assert_eq!(detail.in_amount_cents, in_amount_cents, "转入金额不符");
    assert_eq!(detail.fee_cents, fee_cents, "手续费不符");
}

/// 转出腿逐批次消耗记录（修改回退/删除精确回补的唯一依据）：编辑回退后不得
/// 追加残留（旧消耗行须随清理删除）。
#[then(expr = "转换转入 {string} 的消耗记录应为 {int} 条")]
fn assert_conversion_row_count(world: &mut LedgerWorld, to_symbol: String, expected: i64) {
    let id = convert_txn_id(world, &to_symbol);
    let count: i64 = world_conn!(world)
        .query_row(
            "SELECT COUNT(*) FROM security_lot_conversions WHERE transaction_id=?1",
            params![id],
            |r| r.get(0),
        )
        .expect("读取转换消耗记录失败");
    assert_eq!(count, expected, "转换转入 {to_symbol} 的消耗记录条数不符");
}

/// 全库已实现盈亏合计（分）：转换零盈亏（不写卖出匹配）与全平仓 Σ 闭合的旅程锚。
#[then(expr = "全库已实现盈亏合计应为 {int}")]
fn assert_library_realized_pnl(world: &mut LedgerWorld, expected: i64) {
    let total: i64 = world_conn!(world)
        .query_row(
            "SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales",
            [],
            |r| r.get(0),
        )
        .expect("读取全库已实现盈亏失败");
    assert_eq!(total, expected, "全库已实现盈亏合计不符");
}

/// 验收 2（issue #982）：全平仓后 Σ 已实现盈亏 = Σ 卖出金额 − Σ 买入金额——
/// 转换零盈亏使全库闭合式在转换在场时保持原形（转换中性）。
#[then(expr = "全库 Σ 已实现盈亏应等于 Σ 卖出金额 − Σ 买入金额")]
fn assert_realized_pnl_closure(world: &mut LedgerWorld) {
    let (realized, sells, buys): (i64, i64, i64) = world_conn!(world)
        .query_row(
            "SELECT (SELECT COALESCE(SUM(realized_pnl_cents),0) FROM security_lot_sales), \
                    (SELECT COALESCE(SUM(amount_cents),0) FROM transactions \
                     WHERE kind='sell' AND is_deleted=0), \
                    (SELECT COALESCE(SUM(amount_cents),0) FROM transactions \
                     WHERE kind='buy' AND is_deleted=0)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("读取闭合不变式三项失败");
    assert_eq!(
        realized,
        sells - buys,
        "闭合不变式：Σ 已实现盈亏应等于 Σ 卖出金额 − Σ 买入金额（转换中性）"
    );
}
