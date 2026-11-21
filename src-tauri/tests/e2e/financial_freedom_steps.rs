use cucumber::{given, then, when};

use ledger_accounts::{AccountInput, AccountType, create_account};
use ledger_investment::query_financial_freedom;

use crate::world::LedgerWorld;

#[given(expr = "存在隐藏账户 {string} 类型 {string} 币种 {string} 初始余额 {int}")]
fn create_hidden_account(
    world: &mut LedgerWorld,
    name: String,
    kind: String,
    currency: String,
    initial_balance: i64,
) {
    let id = create_account(
        &world_conn!(world),
        AccountInput {
            name: name.clone(),
            kind: kind.parse::<AccountType>().expect("非法账户类型"),
            currency_code: currency,
            initial_balance_cents: Some(initial_balance),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .expect("创建隐藏账户失败");
    world_conn!(world)
        .execute("UPDATE accounts SET is_hidden=1 WHERE id=?1", [id.as_str()])
        .expect("置隐藏标志失败");
    world.account_name_to_id.insert(name, id);
}

#[when(expr = "查询财务自由度")]
fn query_financial_freedom_step(world: &mut LedgerWorld) {
    match query_financial_freedom(&world_conn!(world)) {
        Ok(overview) => {
            world.asset.last_financial_freedom = Some(overview);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.asset.last_financial_freedom = None;
        }
    }
}

fn overview_of(world: &LedgerWorld) -> &ledger_investment::FinancialFreedomOverview {
    world
        .asset
        .last_financial_freedom
        .as_ref()
        .expect("未查询到财务自由度总览")
}

#[then(expr = "自由度分子应为 {int}")]
fn assert_numerator(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(
        overview_of(world).numerator_cents,
        expected,
        "可投资资产分子不符"
    );
}

#[then(expr = "自由度分母应为 {int}")]
fn assert_denominator(world: &mut LedgerWorld, expected: i64) {
    assert_eq!(
        overview_of(world).denominator_cents,
        expected,
        "年度预算总额分母不符"
    );
}

#[then(expr = "自由度应为 {float}")]
fn assert_ratio(world: &mut LedgerWorld, expected: f64) {
    let actual = overview_of(world).ratio;
    assert!(
        (actual - expected).abs() < 1e-9,
        "自由度不符: 期望 {expected}, 实际 {actual}"
    );
}

#[then(expr = "覆盖年数应为 {float}")]
fn assert_coverage_years(world: &mut LedgerWorld, expected: f64) {
    let actual = overview_of(world).coverage_years;
    assert!(
        (actual - expected).abs() < 1e-9,
        "覆盖年数不符: 期望 {expected}, 实际 {actual}"
    );
}

#[then(expr = "本位币应为 {string}")]
fn assert_native_currency(world: &mut LedgerWorld, expected: String) {
    assert_eq!(overview_of(world).native_currency, expected, "本位币不符");
}
