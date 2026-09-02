//! 财务自由度口径 e2e 步骤定义（issue #343 / ADR-0048）。
//!
//! 分子 = 可投资资产（Σ 持仓市值 + Σ 投资账户余额，折算本位币，排除隐藏账户），
//! 分母 = 年度预算总额（Σ 月度预算 × 12 + Σ 年度预算，全部未删除、无窗口）。
//! 夹具复用既有步骤（dashboard_steps 的标的/现价/买入、accounts_steps 的账户、
//! scheduled_steps 的汇率、budget_steps 的预算分类、manual_quote_steps 的录价），
//! 查询经核心函数 `query_financial_freedom`（命令层同款，不经 IPC 壳）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::investment::query_financial_freedom;

use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given：隐藏投资账户夹具（隐藏账户不进分子的场景专用）
// ---------------------------------------------------------------------------

/// 直插一个隐藏账户行并注册名称→id 映射（`is_hidden=1`，含黑洞同款可见性）。
#[given(expr = "存在隐藏账户 {string} 类型 {string} 币种 {string} 初始余额 {int}")]
fn create_hidden_account(
    world: &mut LedgerWorld,
    name: String,
    kind: String,
    currency: String,
    initial_balance: i64,
) {
    let id = new_uuid();
    let now = now_iso();
    world_conn!(world)
        .execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0,1)",
            params![id, name, kind, currency, initial_balance, now, now, 1, device_id()],
        )
        .unwrap();
    world.account_name_to_id.insert(name, id);
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "查询财务自由度")]
fn query_financial_freedom_step(world: &mut LedgerWorld) {
    match query_financial_freedom(&world_conn!(world)) {
        Ok(overview) => {
            world.last_financial_freedom = Some(overview);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.last_financial_freedom = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

/// 取最近一次自由度快照（各 Then 断言共用）。
fn overview_of(world: &LedgerWorld) -> &tauri_app_lib::models::FinancialFreedomOverview {
    world
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
