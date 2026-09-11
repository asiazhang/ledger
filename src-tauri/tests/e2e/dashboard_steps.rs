//! 首页财务全貌（净资产跨币种合计）e2e 步骤定义（issue #142）。

use cucumber::{given, then, when};

use tauri_app_lib::dashboard::query_dashboard_overview;
use tauri_app_lib::investment::prices::{MarketPriceWrite, upsert_market_price};
use tauri_app_lib::investment::{InstrumentInput, InstrumentType, create_instrument};
use tauri_app_lib::transaction::{TransactionKind, create_transaction_internal};

use crate::common::instrument_id_by_symbol;
use crate::step_inputs::trade_input;
use crate::world::LedgerWorld;

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 经投资域核心创建入口建标的字典行（聚合测试只需要 id/symbol/币种；
/// #763 旁路归零；市场缺省 unknown，与原直插同值）。
#[given(expr = "存在标的 {string} 币种 {string}")]
fn create_instrument_fixture(world: &mut LedgerWorld, symbol: String, currency: String) {
    let input = InstrumentInput {
        symbol: symbol.clone(),
        kind: InstrumentType::Stock,
        name: Some(symbol),
        currency_code: currency,
        market: None,
    };
    world
        .db
        .write(|conn| create_instrument(conn, input))
        .expect("新建标的失败");
}

/// 插入标的市场现价（market_prices 每标的仅保留最新一行）：经投资域现价缓存
/// 写入单点 [`upsert_market_price`]（#764 旁路收敛）；`priced_at` 为行情日期
/// （域时刻，无断言语义，取非 FIXED_NOW 日期段）；source 落 NULL、股票无
/// nav_date，与原直插形状一致。
#[given(expr = "标的 {string} 现价 {int} 币种 {string}")]
fn set_market_price(world: &mut LedgerWorld, symbol: String, price: i64, currency: String) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    upsert_market_price(
        &world_conn!(world),
        &MarketPriceWrite {
            instrument_id: &instrument_id,
            price_cents: price,
            currency_code: &currency,
            priced_at: "2025-06-01",
            nav_date: None,
            source: None,
        },
    )
    .expect("标的现价夹具：写入失败");
}

/// 经行为层创建一笔买入交易（建立持仓批次），走与真实写路径一致的 plan → insert → apply。
/// 同时注册 Given/When：场景中可在创建账户（When）之前或之后使用。
#[given(expr = "已买入 标的 {string} 数量 {int} 单价 {int} 到账户 {string}")]
#[when(expr = "已买入 标的 {string} 数量 {int} 单价 {int} 到账户 {string}")]
fn buy_instrument(
    world: &mut LedgerWorld,
    symbol: String,
    quantity: i64,
    price_cents: i64,
    account_name: String,
) {
    let instrument_id = instrument_id_by_symbol(&world_conn!(world), &symbol);
    let account_id = world.account_id(&account_name);
    // 买入交易以账户币种成交、行金额由 prepare 按数量×单价重算（L1 买卖工厂
    // 置零语义）；fixture 入参与真实写路径一致，不依赖 prepare 兕底覆盖。
    let input = trade_input(
        TransactionKind::Buy,
        &instrument_id,
        quantity as f64,
        price_cents,
        &account_id,
        "2026-01-10",
    );
    let result = create_transaction_internal(&world_conn!(world), input);
    assert!(result.is_ok(), "创建买入交易失败: {:?}", result.err());
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "查询净资产总览")]
fn query_net_worth(world: &mut LedgerWorld) {
    match query_dashboard_overview(&world_conn!(world)) {
        Ok(overview) => {
            world.report.last_overview = Some(overview);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.report.last_overview = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "净资产应为 {int}")]
fn assert_net_worth(world: &mut LedgerWorld, expected: i64) {
    let overview = world
        .report
        .last_overview
        .as_ref()
        .expect("未查询到净资产总览");
    assert_eq!(overview.net_worth_cents, expected, "净资产合计不符");
}

#[then(expr = "非投资账户余额合计应为 {int}")]
fn assert_accounts_balance(world: &mut LedgerWorld, expected: i64) {
    let overview = world
        .report
        .last_overview
        .as_ref()
        .expect("未查询到净资产总览");
    assert_eq!(
        overview.accounts_balance_cents, expected,
        "非投资账户余额合计不符"
    );
}

#[then(expr = "实物资产估值合计应为 {int}")]
fn assert_physical_assets_value(world: &mut LedgerWorld, expected: i64) {
    let overview = world
        .report
        .last_overview
        .as_ref()
        .expect("未查询到净资产总览");
    assert_eq!(
        overview.physical_assets_value_cents, expected,
        "实物资产估值合计不符"
    );
}

#[then(expr = "持仓市值合计应为 {int}")]
fn assert_holdings_value(world: &mut LedgerWorld, expected: i64) {
    let overview = world
        .report
        .last_overview
        .as_ref()
        .expect("未查询到净资产总览");
    assert_eq!(
        overview.holdings_market_value_cents, expected,
        "持仓市值合计不符"
    );
}
