//! 投资概览命令面集成测试（spec #1532 / issue #1536，ADR-0087 壳行为权威层）。
//!
//! 断言面 = 命令契约与返回值形状（spec #1532 测试决策显式要求「壳层 API 集成
//! 测试：新命令的契约与返回值形状（含缺价持仓计数与本位币字段）」）：命令注册
//! 面直调（`#[tokio::test]` + mock 应用）、返回值形状（两腿与合计、本位币字段、
//! 未计入持仓计数、有无投资账户）、多币种折本位币与隐藏账户排除、缺汇率的码化
//! 错误契约。口径细节的展开（恒等绑定、两腿折算的负向判据等）仍归域单测
//! （`crates/investment/src/tests/overview.rs`），本层不重复其全部场景。
//!
//! 造数纪律（ADR-0087 决策 3）：静态基线走统一测试数据库工厂，行为前置经公开
//! 写入口（交易写入 + 投资域价格写入单点）；`is_hidden` 无公开入口可表达，库内
//! 直置（已登记先例：e2e 隐藏账户步骤，issue #763/#764）。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
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
use tauri_app_lib::test_support::{seed_account, seed_exchange_rate, seed_instrument};

/// mock 应用 + 独立临时目录文件库（`readonly_connection` / `instrument_sync`
/// 同款现场：产品建连缝 + 交易域接缝接线，本位币读取钩子随此装入）。
fn device_app(tag: &str) -> tauri::App<tauri::test::MockRuntime> {
    // 写路径副作用接线（与测试工厂 / cross_book_summary 现场同款：交易写入的
    // 余额缓存重算实现随此装入；未接线时写入即码化拒绝）。
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = std::env::temp_dir().join(format!(
        "ledger-overview-it-{tag}-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).expect("临时目录应可建");
    let app = tauri::test::mock_app();
    app.manage(db::open_db_in(&dir).expect("文件库应可开"));
    app
}

/// 买入输入（公开写入口造数：建仓批次与现金腿由产品代码落库，非裸插）。
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

/// 卖出输入（已实现盈亏腿造数）。
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
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: 0,
        currency_code: currency.into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
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
    }
}

/// 现金分红输入（累计收益第三腿造数）：金额 = `amount_cents`，币种须与到账
/// 账户一致（写路径守卫）。
fn dividend_input(
    account_id: &str,
    instrument_id: &str,
    amount_cents: i64,
    currency: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Dividend,
        amount_cents,
        currency_code: currency.into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
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
    }
}

/// 种入一行现价缓存（投资域价格写入单点，非裸 SQL）。
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

/// 契约：两腿与合计、本位币、未计入持仓计数与有无投资账户一并返回；
/// 缺价持仓跳过但计入未计入数（命令面形状，前端类型镜像此形状）。
#[tokio::test]
async fn investment_overview_returns_two_legs_and_unpriced_count() {
    let app = device_app("contract");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-1", "境内券商", "investment", "CNY", 100_000);
        seed_instrument(&guard, "inst-priced", "PRICED", "有价标的", "CNY", "sh");
        seed_instrument(&guard, "inst-bare", "BARE", "缺价标的", "CNY", "sh");
        seed_exchange_rate(&guard, "CNY", "CNY", 1.0);
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
        // 买入 1 份 @ 100 元与 3 份 @ 100 元：现金腿 = 初始 100000 − 10000 − 30000。
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
        // inst-bare 无现价 → 市值空值语义跳过，但不静默低估（计数可见）。
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
    // 投资合计三项（#1537）：有价持仓现价 120 元（成本 100 元）→ 市值 120 元、
    // 持仓收益 +20 元；无卖出与分红 → 累计收益 = 持仓收益。
    assert_eq!(overview.total_market_value_cents, 12_000);
    assert_eq!(overview.unrealized_pnl_cents, 2_000);
    assert_eq!(overview.cumulative_pnl_cents, 2_000);
    assert_eq!(overview.missing_price_holding_count, 1);
    assert!(overview.has_investment_account);

    // 线格式契约（前端 `InvestmentOverview` 类型镜像此字段集）：改动即须同步前端。
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

/// 投资合计三项折全局默认币种（#1537 验收判据的 API 层半边）：买入 + 卖出（已实现）
/// + 分红 + 现价，三项均折本位币；契约细节与恒等绑定归域单测，此处只断言
/// 命令面返回值。
#[tokio::test]
async fn investment_overview_totals_fold_realized_and_dividends() {
    let app = device_app("totals");
    let conn = app.state::<DbState>().conn.clone();
    {
        let guard = conn.lock().expect("种子写入锁应可取");
        seed_account(&guard, "acc-usd", "美股券商", "investment", "USD", 0);
        seed_instrument(&guard, "inst-x", "XX", "标的X", "USD", "nasdaq");
        seed_exchange_rate(&guard, "USD", "CNY", 7.0);
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
        // 买 2 股 @ $100 → 卖 1 股 @ $120（已实现 +$20）→ 分红 $30；现价 $150 →
        // 余 1 股成本 $100：市值 $150、未实现 +$50。
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

/// 多币种折全局默认币种 + 隐藏账户的现金与持仓一并排除（验收判据的 API 层半边）。
#[tokio::test]
async fn investment_overview_folds_multi_currency_and_excludes_hidden() {
    let app = device_app("hidden");
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
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
        // 隐藏账户里的持仓（市值 2 × 150 美元 = 30000 美分）与现金一并不计入。
        create_transaction_internal(
            &guard,
            buy_input("acc-hidden", "inst-hid", 2.0, 1_000_000, "USD"),
        )
        .expect("建仓应成功");
        seed_price(&guard, "inst-hid", 1_500_000, "USD");
        // `is_hidden` 无公开入口可表达（黑洞账户仅由种子预置），库内直置留置。
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
    // 投资合计三项同面排除隐藏账户（页面级边界「同 InvestableAssets 口径」）。
    assert_eq!(overview.total_market_value_cents, 0);
    assert_eq!(overview.unrealized_pnl_cents, 0);
    assert_eq!(overview.cumulative_pnl_cents, 0);
    assert_eq!(overview.missing_price_holding_count, 0);
    assert!(overview.has_investment_account);
}

/// 没有投资账户：照常返回 0 与「还没有投资账户」的引导事实（不隐藏功能、不报错）。
#[tokio::test]
async fn investment_overview_without_investment_account_returns_zero() {
    let app = device_app("empty");
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

/// 缺折算汇率：码化错误上抛（前端据此卡内警告 + 重试，不显示半截数字）。
#[tokio::test]
async fn investment_overview_missing_rate_returns_coded_error() {
    let app = device_app("no-rate");
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
