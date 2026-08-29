//! 首页财务全貌（净资产跨币种合计）e2e 步骤定义（issue #142）。

use cucumber::{given, then, when};
use rusqlite::params;

use tauri_app_lib::commands::dashboard::query_dashboard_overview;
use tauri_app_lib::commands::transactions::insert_transaction;
use tauri_app_lib::db::{device_id, new_uuid, now_iso};
use tauri_app_lib::models::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

use crate::world::LedgerWorld;

/// 按标的代码查 instrument id（同一连接内刚插入，必存在）。
fn instrument_id(conn: &rusqlite::Connection, symbol: &str) -> String {
    conn.query_row(
        "SELECT id FROM instruments WHERE symbol=?1",
        params![symbol],
        |r| r.get(0),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// Given
// ---------------------------------------------------------------------------

/// 直接插入金融工具字典行（投资域字典，聚合测试只需要 id/symbol/币种）。
#[given(expr = "存在标的 {string} 币种 {string}")]
fn create_instrument(world: &mut LedgerWorld, symbol: String, currency: String) {
    let now = now_iso();
    world
        .conn
        .execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?2,?3,'unknown',?4,?4,1,?5)",
            params![new_uuid(), symbol, currency, now, device_id()],
        )
        .unwrap();
}

/// 插入标的市场现价（market_prices 每标的仅保留最新一行）。
#[given(expr = "标的 {string} 现价 {int} 币种 {string}")]
fn set_market_price(world: &mut LedgerWorld, symbol: String, price: i64, currency: String) {
    let instrument_id = instrument_id(&world.conn, &symbol);
    let now = now_iso();
    world
        .conn
        .execute(
            "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,'2026-01-01',?5,?5,1,?6)",
            params![new_uuid(), instrument_id, price, currency, now, device_id()],
        )
        .unwrap();
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
    let instrument_id = instrument_id(&world.conn, &symbol);
    let account_id = world.account_id(&account_name);
    // 买入交易以账户币种成交：fixture 入参与真实写路径一致，不依赖 prepare 兕底覆盖。
    let currency_code: String = world
        .conn
        .query_row(
            "SELECT currency_code FROM accounts WHERE id=?1",
            params![account_id],
            |r| r.get(0),
        )
        .unwrap();
    let input = TransactionInput {
        kind: TransactionKind::Buy,
        amount_cents: quantity * price_cents,
        currency_code,
        account_id,
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id),
        quantity: Some(quantity as f64),
        price_cents: Some(price_cents),
        fee_cents: Some(0),
        idempotency_key: None,
    };
    let result = insert_transaction(&world.conn, input);
    assert!(result.is_ok(), "创建买入交易失败: {:?}", result.err());
}

// ---------------------------------------------------------------------------
// When
// ---------------------------------------------------------------------------

#[when(expr = "查询净资产总览")]
fn query_net_worth(world: &mut LedgerWorld) {
    match query_dashboard_overview(&world.conn) {
        Ok(overview) => {
            world.last_overview = Some(overview);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e.to_string());
            world.last_overview = None;
        }
    }
}

// ---------------------------------------------------------------------------
// Then
// ---------------------------------------------------------------------------

#[then(expr = "净资产应为 {int}")]
fn assert_net_worth(world: &mut LedgerWorld, expected: i64) {
    let overview = world.last_overview.as_ref().expect("未查询到净资产总览");
    assert_eq!(overview.net_worth_cents, expected, "净资产合计不符");
}

#[then(expr = "非投资账户余额合计应为 {int}")]
fn assert_accounts_balance(world: &mut LedgerWorld, expected: i64) {
    let overview = world.last_overview.as_ref().expect("未查询到净资产总览");
    assert_eq!(
        overview.accounts_balance_cents, expected,
        "非投资账户余额合计不符"
    );
}

#[then(expr = "持仓市值合计应为 {int}")]
fn assert_holdings_value(world: &mut LedgerWorld, expected: i64) {
    let overview = world.last_overview.as_ref().expect("未查询到净资产总览");
    assert_eq!(
        overview.holdings_market_value_cents, expected,
        "持仓市值合计不符"
    );
}
