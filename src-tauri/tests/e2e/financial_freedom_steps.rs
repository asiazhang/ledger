//! 财务自由度口径 e2e 步骤定义（issue #343 / ADR-0048）。
//!
//! 分子 = 可投资资产（Σ 持仓市值 + Σ 投资账户余额，折算本位币，排除隐藏账户），
//! 分母 = 年度预算总额（Σ 月度预算 × 12 + Σ 年度预算，全部未删除、无窗口）。
//! 夹具复用既有步骤（dashboard_steps 的标的/现价/买入、accounts_steps 的账户、
//! scheduled_steps 的汇率、budget_steps 的预算分类、manual_quote_steps 的录价），
//! 查询经核心函数 `query_financial_freedom`（命令层同款，不经 IPC 壳）。

use cucumber::{given, then, when};

use tauri_app_lib::accounts::{AccountInput, AccountType, create_account};
use tauri_app_lib::investment::query_financial_freedom;

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given：隐藏投资账户夹具（隐藏账户不进分子的场景专用）
// ---------------------------------------------------------------------------

/// 创建隐藏账户（`is_hidden=1`，含黑洞同款可见性）并注册名称→id 映射：创建经
/// 账户域公开入口（余额缓存行不变量由产品代码保证，#763 旁路归零）；
/// `is_hidden` 无公开入口可表达（黑洞账户仅由种子预置），库内状态直置后仅在
/// 该属性上留置（#764 已登记例外）。
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
        },
    )
    .expect("创建隐藏账户失败");
    world_conn!(world)
        .execute("UPDATE accounts SET is_hidden=1 WHERE id=?1", [id.as_str()])
        .expect("置隐藏标志失败");
    world.account_name_to_id.insert(name, id);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

/// 取最近一次自由度快照（各 Then 断言共用）。
fn overview_of(world: &LedgerWorld) -> &tauri_app_lib::investment::FinancialFreedomOverview {
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
