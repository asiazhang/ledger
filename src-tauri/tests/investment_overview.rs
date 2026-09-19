//! 投资概览命令面集成测试（spec #1532 / issue #1536，ADR-0087 壳行为权威层）。
//!
//! 覆盖壳行为：命令注册面直调（`#[tokio::test]` + mock 应用）、返回值形状
//! （两腿与合计、本位币字段、未计入持仓计数、有无投资账户）与缺汇率的码化错误
//! 契约。口径细节（隐藏账户排除、恒等绑定、折本位币的负向判据）归域单测
//! （`crates/investment/src/tests/overview.rs`），此处只钉用户可见的契约面。

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
use tauri_app_lib::commands::investment::investment_overview;
use tauri_app_lib::test_support::{seed_account, seed_exchange_rate, seed_instrument};

/// mock 应用 + 独立临时目录文件库（`readonly_connection` / `instrument_sync`
/// 同款现场：产品建连缝 + 交易域接缝接线，本位币读取钩子随此装入）。
fn device_app(tag: &str) -> tauri::App<tauri::test::MockRuntime> {
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

/// 种入一笔记账锚点（持仓批次的外键目标）：`transactions` + `security_transactions`
/// + `security_lots` 三行，金额与批次自洽（`instrument_sync` 同款裸插先例）。
fn seed_lot(
    conn: &Connection,
    txn_id: &str,
    lot_id: &str,
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
) {
    let amount_cents = (quantity * price_cents as f64 / 100.0).round() as i64;
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',?2,'CNY',?2,?3,'2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        rusqlite::params![txn_id, amount_cents, account_id],
    )
    .expect("种子交易应落库");
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',?3,?4,0)",
        rusqlite::params![txn_id, instrument_id, quantity, price_cents],
    )
    .expect("种子扩展行应落库");
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,'CNY','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        rusqlite::params![lot_id, account_id, instrument_id, txn_id, quantity, price_cents],
    )
    .expect("种子批次应落库");
}

/// 种入一行现价缓存（`v_holdings` 据现价算市值；基金行另带净值日期，本例无）。
fn seed_price(conn: &Connection, instrument_id: &str, price_cents: i64, currency: &str) {
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,'2026-01-10T00:00:00Z',NULL,'2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        rusqlite::params![db::new_uuid(), instrument_id, price_cents, currency],
    )
    .expect("种子现价应落库");
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
        // 买入 1 份 @ 100 元与 3 份 @ 100 元：现金腿 = 初始 100000 − 10000 − 30000。
        seed_lot(
            &guard,
            "txn-1",
            "lot-1",
            "acc-1",
            "inst-priced",
            1.0,
            1_000_000,
        );
        seed_lot(
            &guard,
            "txn-2",
            "lot-2",
            "acc-1",
            "inst-bare",
            3.0,
            1_000_000,
        );
        seed_price(&guard, "inst-priced", 1_200_000, "CNY");
        // inst-bare 无现价 → 市值空值语义跳过，但不静默低估（计数可见）。
        ledger_accounts::balance::refresh_all_account_balances(&guard).expect("余额缓存应可回填");
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
            "missing_price_holding_count": 1,
            "has_investment_account": true,
        })
    );
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
        seed_account(&guard, "acc-usd", "美股券商", "investment", "USD", 80_000);
        seed_exchange_rate(&guard, "CNY", "CNY", 1.0);
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
