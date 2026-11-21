use std::collections::HashMap;

use cucumber::{then, when};
use rusqlite::params;

use ledger_accounts::list_account_balances_for_api;
use ledger_transaction::TransactionInput;

use crate::common::{capture_expected_error, instrument_id_by_symbol, query_all_transactions};
use crate::step_inputs::convert_input;
use crate::step_verbs::{
    create_transaction_verb, delete_transaction_verb, try_delete_transaction_verb,
    update_transaction_verb,
};
use crate::world::LedgerWorld;

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

#[when(
    expr = "按确认单于 {string} 转换 {string} 份额 {float} 为 {string} 份额 {float} 转出金额 {int} 转入金额 {int} 手续费 {int} 到投资账户 {string}"
)]
#[allow(clippy::too_many_arguments)]
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

#[when(expr = "删除转换（转入 {string}）")]
fn delete_convert(world: &mut LedgerWorld, to_symbol: String) {
    let id = convert_txn_id(world, &to_symbol);
    delete_transaction_verb(world, &id);
    world.txn.transactions_list = query_all_transactions(&world_conn!(world));
}

#[when(expr = "尝试删除买入交易 {string}")]
fn try_delete_buy(world: &mut LedgerWorld, symbol: String) {
    let id = crate::transactions_edit_steps::trade_txn_id(world, &symbol, "buy");
    let result = try_delete_transaction_verb(world, &id);
    capture_expected_error(world, result.map(|_| ()));
}

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

#[then(expr = "转换转入 {string} 的手续费应为 {int}")]
fn assert_convert_fee(world: &mut LedgerWorld, to_symbol: String, expected: i64) {
    let detail = convert_detail(world, &to_symbol);
    assert_eq!(
        detail.fee_cents, expected,
        "转换转入 {to_symbol} 的手续费不符"
    );
}

#[then(
    expr = "转换转入 {string} 的明细应为 转出 {string} 份额 {float} 转入份额 {float} 转出金额 {int} 转入金额 {int} 手续费 {int}"
)]
#[allow(clippy::too_many_arguments)]
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
