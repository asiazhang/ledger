//! 投资明细列表 BDD 步骤（ADR-0135 / issue #1778）：投资页「明细」页签的
//! 用户旅程——买卖与分红造数走 L1 输入工厂 + 行为层公开写入口（step_verbs），
//! 明细命令按序返回投资投影行（行序 / 类型 / 行金额 / 到账账户 / 标的；载荷
//! 字段逐项断言归域单测），并回归钉报表收入口径不动（分红仍计收入净额）。

use cucumber::{then, when};

use ledger_investment::InvestmentTransactionListFilter;

use crate::common::instrument_id_by_symbol;
use crate::step_inputs::{buy_input, dividend_input, parse_kind, sell_input};
use crate::step_verbs::create_transaction_verb;
use crate::world::LedgerWorld;

/// 买入标的（单价权威形态，非基金）：数量 / 单价热点，金额由后端按数量 × 单价重算
/// （L1 工厂置零）。经行为层公开创建入口写入。
#[when(expr = "买入标的 {string} 数量 {int} 单价 {int} 到投资账户 {string} 日期 {string}")]
fn buy_instrument(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
    date: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    let account_id = world.account_id(&account_name);
    create_transaction_verb(
        world,
        buy_input(
            &instrument_id,
            quantity as f64,
            Some(price_cents),
            &account_id,
            &date,
        ),
    );
}

/// 卖出标的（单价权威形态）：FIFO 匹配在用批次。
#[when(expr = "卖出标的 {string} 数量 {int} 单价 {int} 从投资账户 {string} 日期 {string}")]
fn sell_instrument(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
    date: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    let account_id = world.account_id(&account_name);
    create_transaction_verb(
        world,
        sell_input(
            &instrument_id,
            quantity as f64,
            Some(price_cents),
            &account_id,
            &date,
        ),
    );
}

/// 创建现金分红（ADR-0109）：归属某标的的现金收入，到账账户任意在用账户。
#[when(expr = "创建现金分红 {int} 到账户 {string} 归属标的 {string} 日期 {string}")]
fn create_dividend(
    world: &mut LedgerWorld,
    amount_cents: i64,
    account_name: String,
    symbol: String,
    date: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    let account_id = world.account_id(&account_name);
    create_transaction_verb(
        world,
        dividend_input(amount_cents, &account_id, &instrument_id, &date),
    );
}

/// 查询投资明细列表（缺省过滤）：与 IPC 命令同一域函数（域入口单点）。
#[when(expr = "查询投资明细列表")]
fn query_investment_ledger_tab(world: &mut LedgerWorld) {
    world.asset.last_investment_transactions = Some(
        ledger_investment::list_investment_transactions(
            &world_conn!(world),
            &InvestmentTransactionListFilter::default(),
        )
        .expect("投资明细列表应返回"),
    );
}

fn ledger_tab_rows(world: &LedgerWorld) -> &[ledger_investment::InvestmentTransactionRow] {
    world
        .asset
        .last_investment_transactions
        .as_ref()
        .expect("先执行 查询投资明细列表")
        .items
        .as_slice()
}

/// 明细行数（投资投影行集 = 五种投资 kind，通用 kind 不入集）。
#[then(expr = "投资明细应为 {int} 行")]
fn assert_ledger_tab_row_count(world: &mut LedgerWorld, count: usize) {
    let rows = ledger_tab_rows(world);
    assert_eq!(rows.len(), count, "投资明细行数不符：实际 {rows:?}");
}

/// 第 n 行的类型与行金额锚点（dividend = 现金腿、convert = 结转成本，其余 =
/// 数量 × 单价 ± 手续费的行金额）。
#[then(expr = "投资明细第 {int} 行类型应为 {string} 金额应为 {int}")]
fn assert_ledger_tab_row_kind_amount(
    world: &mut LedgerWorld,
    index: usize,
    kind: String,
    amount_cents: i64,
) {
    let row = ledger_tab_rows(world)
        .get(index - 1)
        .unwrap_or_else(|| panic!("投资明细第 {index} 行不存在"));
    assert_eq!(row.kind, parse_kind(&kind), "投资明细第 {index} 行类型不符");
    assert_eq!(
        row.amount_cents, amount_cents,
        "投资明细第 {index} 行金额不符"
    );
}

/// 第 n 行的到账账户（dividend 账户端 = 到账账户，任意在用账户）与归属标的代码。
#[then(expr = "投资明细第 {int} 行到账账户应匹配账户 {string} 标的代码应为 {string}")]
fn assert_ledger_tab_row_arrival_and_symbol(
    world: &mut LedgerWorld,
    index: usize,
    account_name: String,
    symbol: String,
) {
    let row = ledger_tab_rows(world)
        .get(index - 1)
        .unwrap_or_else(|| panic!("投资明细第 {index} 行不存在"));
    let account_id = world.account_id(&account_name);
    assert_eq!(
        row.account_id, account_id,
        "投资明细第 {index} 行账户端不符（dividend 到账账户即账户端）"
    );
    assert_eq!(row.symbol, symbol, "投资明细第 {index} 行归属标的不符");
}
