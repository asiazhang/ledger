#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use rusqlite::Connection;
use tauri::Manager;

use ledger_infra::db::{self, DbState};
use ledger_investment::prices::{MarketPriceWrite, upsert_market_price};
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri_app_lib::commands::investment::investment_overview;
use tauri_app_lib::test_support::{
    seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_instrument,
};

fn device_app(
    tag: &str,
) -> (
    tauri::App<tauri::test::MockRuntime>,
    tauri_app_lib::test_support::ScratchDir,
) {
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = tauri_app_lib::test_support::ScratchDir::new(&format!("overview-it-{tag}"));
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));
    (app, dir)
}

fn buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price_cents: i64,
    currency: &str,
) -> TransactionInput {
    trade_input(
        TransactionKind::Buy,
        account_id,
        instrument_id,
        qty,
        price_cents,
        currency,
    )
}

fn sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price_cents: i64,
    currency: &str,
) -> TransactionInput {
    trade_input(
        TransactionKind::Sell,
        account_id,
        instrument_id,
        qty,
        price_cents,
        currency,
    )
}

fn trade_input(
    kind: TransactionKind,
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price_cents: i64,
    currency: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: 0,
        currency_code: currency.into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price_cents),
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

fn dividend_input(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    currency: &str,
) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: currency.into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: None,
        date: "2026-02-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: None,
        price_cents: None,
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

fn seed_price(conn: &Connection, instrument_id: &str, price_cents: i64, currency: &str) {
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents,
            currency_code: currency,
            priced_at: &db::now_iso(),
            nav_date: None,
            source: Some("eastmoney"),
        },
    )
    .expect("现价缓存写入应成功");
}

#[tokio::test]
async fn investment_overview_returns_two_legs_and_unpriced_count() {
    let (app, _dir) = device_app("contract");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-1", "境内券商", "investment", "CNY", 100_000);
        seed_instrument(&guard, "inst-priced", "PRICED", "有价标的", "CNY", "sh");
        seed_instrument(&guard, "inst-bare", "BARE", "缺价标的", "CNY", "sh");
        seed_exchange_rate(&guard, "CNY", "CNY", 1.0);
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
        create_transaction_internal(
            &guard,
            buy_input("acc-1", "inst-priced", 1.0, 1_000_000, "CNY"),
        )
        .expect("建仓应成功");
        create_transaction_internal(
            &guard,
            buy_input("acc-1", "inst-bare", 3.0, 1_000_000, "CNY"),
        )
        .expect("建仓应成功");
        seed_price(&guard, "inst-priced", 1_200_000, "CNY");
    }

    let overview = investment_overview(app.state::<DbState>())
        .await
        .expect("投资概览应返回");
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(overview.investment_cash_cents, 60_000);
    assert_eq!(overview.holdings_market_value_cents, 12_000);
    assert_eq!(
        overview.investable_assets_cents,
        overview.investment_cash_cents + overview.holdings_market_value_cents
    );
    assert_eq!(overview.total_market_value_cents, 12_000);
    assert_eq!(overview.unrealized_pnl_cents, 2_000);
    assert_eq!(overview.cumulative_pnl_cents, 2_000);
    assert_eq!(overview.missing_price_holding_count, 1);
    assert!(overview.has_investment_account);

    assert_eq!(
        serde_json::to_value(&overview).expect("应可序列化"),
        serde_json::json!({
            "native_currency": "CNY",
            "investable_assets_cents": 72_000,
            "investment_cash_cents": 60_000,
            "holdings_market_value_cents": 12_000,
            "total_market_value_cents": 12_000,
            "unrealized_pnl_cents": 2_000,
            "cumulative_pnl_cents": 2_000,
            "missing_price_holding_count": 1,
            "has_investment_account": true,
        })
    );
}

#[tokio::test]
async fn investment_overview_totals_fold_realized_and_dividends() {
    let (app, _dir) = device_app("totals");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-usd", "美股券商", "investment", "USD", 0);
        seed_instrument(&guard, "inst-x", "XX", "标的X", "USD", "nasdaq");
        seed_exchange_rate(&guard, "USD", "CNY", 7.0);
        seed_fx_history_weeks(
            &guard,
            "USD",
            "CNY",
            7.0,
            &["2026-01-10", "2026-01-20", "2026-02-10"],
        );
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
        create_transaction_internal(
            &guard,
            buy_input("acc-usd", "inst-x", 2.0, 1_000_000, "USD"),
        )
        .expect("建仓应成功");
        create_transaction_internal(
            &guard,
            sell_input("acc-usd", "inst-x", 1.0, 1_200_000, "USD"),
        )
        .expect("卖出应成功");
        create_transaction_internal(&guard, dividend_input("acc-usd", "inst-x", 3_000, "USD"))
            .expect("分红应成功");
        seed_price(&guard, "inst-x", 1_500_000, "USD");
    }

    let overview = investment_overview(app.state::<DbState>())
        .await
        .expect("投资概览应返回");
    assert_eq!(overview.native_currency, "CNY");
    assert_eq!(overview.total_market_value_cents, 105_000, "$150 × 7.0");
    assert_eq!(overview.unrealized_pnl_cents, 35_000);
    assert_eq!(
        overview.cumulative_pnl_cents, 70_000,
        "($50 + $20 + $30) × 7.0"
    );
}

#[tokio::test]
async fn investment_overview_folds_multi_currency_and_excludes_hidden() {
    let (app, _dir) = device_app("hidden");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-usd", "美股券商", "investment", "USD", 100_000);
        seed_account(
            &guard,
            "acc-hidden",
            "秘密券商",
            "investment",
            "USD",
            70_000,
        );
        seed_instrument(&guard, "inst-hid", "HID", "隐藏标的", "USD", "nasdaq");
        seed_exchange_rate(&guard, "USD", "CNY", 7.0);
        seed_fx_history_weeks(
            &guard,
            "USD",
            "CNY",
            7.0,
            &["2026-01-10", "2026-01-20", "2026-02-10"],
        );
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
        create_transaction_internal(
            &guard,
            buy_input("acc-hidden", "inst-hid", 2.0, 1_000_000, "USD"),
        )
        .expect("建仓应成功");
        seed_price(&guard, "inst-hid", 1_500_000, "USD");
        guard
            .execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-hidden'", [])
            .expect("置隐藏标志应成功");
    }

    let overview = investment_overview(app.state::<DbState>())
        .await
        .expect("投资概览应返回");
    assert_eq!(
        overview.investment_cash_cents, 700_000,
        "1000 美元现金折 7.0；隐藏账户现金 700 美元与持仓均排除"
    );
    assert_eq!(overview.holdings_market_value_cents, 0);
    assert_eq!(overview.investable_assets_cents, 700_000);
    assert_eq!(overview.total_market_value_cents, 0);
    assert_eq!(overview.unrealized_pnl_cents, 0);
    assert_eq!(overview.cumulative_pnl_cents, 0);
    assert_eq!(overview.missing_price_holding_count, 0);
    assert!(overview.has_investment_account);
}

#[tokio::test]
async fn investment_overview_without_investment_account_returns_zero() {
    let (app, _dir) = device_app("empty");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-life", "生活现金", "cash", "CNY", 999_999);
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
    }

    let overview = investment_overview(app.state::<DbState>())
        .await
        .expect("无投资账户应照常返回");
    assert_eq!(overview.investable_assets_cents, 0);
    assert_eq!(overview.investment_cash_cents, 0);
    assert_eq!(overview.holdings_market_value_cents, 0);
    assert_eq!(overview.total_market_value_cents, 0);
    assert_eq!(overview.unrealized_pnl_cents, 0);
    assert_eq!(overview.cumulative_pnl_cents, 0);
    assert_eq!(overview.missing_price_holding_count, 0);
    assert!(!overview.has_investment_account);
}

#[tokio::test]
async fn investment_overview_missing_rate_returns_coded_error() {
    let (app, _dir) = device_app("no-rate");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-jpy", "日元券商", "investment", "JPY", 888_000);
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
    }

    let err = investment_overview(app.state::<DbState>())
        .await
        .expect_err("缺汇率应报错");
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );
}
