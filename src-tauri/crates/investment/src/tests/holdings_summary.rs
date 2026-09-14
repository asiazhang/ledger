//! 持仓合计按币种分组读投影与可投资资产分子提取（issue #1196 / ADR-0114）。
//!
//! 领域绑定判据：
//! - `query_holdings_summary_by_currency` 的口径 = 持仓页签合计的既有形态
//!   （issue #902 / ADR-0107 同款「按币种分组、不跨币种折算」）：金额为账户本位币
//!   （`v_holdings` 市值/未实现盈亏列），软删账户由视图内建排除，隐藏账户照常计入
//!   （Holding 口径，issue #217 定案 Q2），缺价/缺汇率的空值跳过、不以零计入；
//! - `query_investable_assets_cents` 是财务自由度分子的单点提取：与
//!   `query_financial_freedom` 的 `numerator_cents` 绑定相等（同一口径两处不得漂移）；
//!   口径归 InvestableAssets 词汇表（排除隐藏账户、生活现金不计入、缺汇率错误上抛）。

use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{create_transaction_internal, TransactionInput};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_exchange_rate, seed_instrument};

/// 币种显式的买入输入构造器（`make_buy_input` 固定 USD，多币种账户按守卫要求
/// 交易币种与账户币种一致，故多币种场景各自构造）。
fn make_buy_input_in(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
    currency: &str,
) -> TransactionInput {
    TransactionInput {
        currency_code: currency.into(),
        ..make_buy_input(account_id, instrument_id, qty, price, fee)
    }
}

/// 种入标的当前行情（现价缓存单行，`v_holdings` 据此算市值与未实现盈亏；
/// 与 cumulative_pnl 测试同款裸插先例）。
fn seed_market_price(
    conn: &rusqlite::Connection,
    instrument_id: &str,
    price_cents: i64,
    currency: &str,
) {
    let now = ledger_infra::db::now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,NULL,?6,?7,?8,?9)",
        rusqlite::params![
            ledger_infra::db::new_uuid(),
            instrument_id,
            price_cents,
            currency,
            now,
            now,
            now,
            1,
            "test"
        ],
    )
    .unwrap();
}

fn totals_for(groups: &[CurrencyHoldingTotals], currency: &str) -> (Option<i64>, Option<i64>) {
    groups
        .iter()
        .find(|g| g.currency_code == currency)
        .map(|g| (g.market_value_cents, g.unrealized_pnl_cents))
        .unwrap_or((None, None))
}

/// 双币种账户各自成组：金额为账户本位币，市值与未实现盈亏按组内求和。
/// 价格刻度为万分之一元（`quantity × price_cents / 100` → 分）。
#[test]
fn holdings_summary_groups_by_account_currency() {
    let conn = open();
    // CNY 账户：买 10 @ 100 元，现价 120 元 → 市值 120_000 分、未实现 20_000 分。
    seed_account(&conn, "acc-cny", "境内账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-cny", "510300", "沪深300ETF", "CNY", "sh");
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-cny", "inst-cny", 10.0, 1_000_000, 0, "CNY"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-cny", 1_200_000, "CNY");
    // USD 账户：买 2 @ 50，现价 60 → 市值 12_000 分、未实现 2_000 分。
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 0);
    seed_instrument(&conn, "inst-usd", "AAPL", "Apple", "USD", "nasdaq");
    // 买入写入的行金额归一需要 USD→CNY 当期汇率（与读无关，v_holdings 金额已是账户币）。
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-usd", "inst-usd", 2.0, 500_000, 0, "USD"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-usd", 600_000, "USD");
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();

    let groups = query_holdings_summary_by_currency(&conn).unwrap();
    assert_eq!(groups.len(), 2, "两币种各自成组");
    assert_eq!(totals_for(&groups, "CNY"), (Some(120_000), Some(20_000)));
    assert_eq!(totals_for(&groups, "USD"), (Some(12_000), Some(2_000)));
}

/// 缺价行采 Holding 空值语义：不计入、不以零计入；组内仍有有价行时其余列照常求和，
/// 全空组不出现。
#[test]
fn holdings_summary_skips_null_values() {
    let conn = open();
    seed_account(&conn, "acc-a", "账户A", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-priced", "PRICED", "有价标的", "CNY", "sh");
    seed_instrument(&conn, "inst-bare", "BARE", "缺价标的", "CNY", "sh");
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-a", "inst-priced", 1.0, 1_000_000, 0, "CNY"),
    )
    .unwrap();
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-a", "inst-bare", 3.0, 1_000_000, 0, "CNY"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-priced", 1_100_000, "CNY");
    // inst-bare 无现价 → 其行两列皆 NULL。

    let groups = query_holdings_summary_by_currency(&conn).unwrap();
    assert_eq!(
        totals_for(&groups, "CNY"),
        (Some(11_000), Some(1_000)),
        "缺价行不进两列合计（市值只含有价行 11_000 分；未实现 = 11_000 − 成本 10_000）"
    );

    // 全空组：只有缺价持仓的币种不出现分组（USD 买入需汇率，读侧不影响）。
    seed_account(&conn, "acc-b", "账户B", "investment", "USD", 0);
    seed_instrument(&conn, "inst-bare2", "BARE2", "缺价标的2", "USD", "nasdaq");
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-b", "inst-bare2", 1.0, 100_000, 0, "USD"),
    )
    .unwrap();
    let groups = query_holdings_summary_by_currency(&conn).unwrap();
    assert!(
        groups.iter().all(|g| g.currency_code != "USD"),
        "全空组不出现，实际 {groups:?}"
    );
}

/// 隐藏账户照常计入（Holding 口径，与可投资资产的隐藏排除互为对照）。
#[test]
fn holdings_summary_includes_hidden_accounts() {
    let conn = open();
    seed_account(&conn, "acc-h", "隐藏投资账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-h", "HID", "隐藏标的", "CNY", "sh");
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-h", "inst-h", 5.0, 1_000_000, 0, "CNY"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-h", 1_000_000, "CNY");
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-h'", [])
        .unwrap();

    let groups = query_holdings_summary_by_currency(&conn).unwrap();
    assert_eq!(
        totals_for(&groups, "CNY"),
        (Some(50_000), Some(0)),
        "隐藏账户持仓照常计入（Holding 口径）"
    );
}

/// 可投资资产 = 投资账户现金 + 持仓市值（折本位币）；生活现金不计入。
#[test]
fn investable_assets_equals_investment_cash_plus_market_value() {
    let conn = open();
    seed_account(&conn, "acc-inv", "投资账户", "investment", "CNY", 200_000);
    seed_account(&conn, "acc-life", "生活现金", "cash", "CNY", 999_999);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    seed_instrument(&conn, "inst-ia", "IA", "标的", "CNY", "sh");
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-inv", "inst-ia", 10.0, 1_000_000, 0, "CNY"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-ia", 1_200_000, "CNY");

    // 买入无出资账户 → 结算账户 = 投资账户自己（ADR-0096）：现金 200_000 − 100_000。
    let total = query_investable_assets_cents(&conn).unwrap();
    assert_eq!(
        total,
        (200_000 - 100_000) + 120_000,
        "现金（买入后净额） + 市值；生活现金不计入"
    );
}

/// 可投资资产排除隐藏账户（InvestableAssets 口径，与持仓合计的隐藏计入对照）。
#[test]
fn investable_assets_excludes_hidden_accounts() {
    let conn = open();
    seed_account(&conn, "acc-h", "隐藏投资账户", "investment", "CNY", 70_000);
    seed_instrument(&conn, "inst-h", "HID", "隐藏标的", "CNY", "sh");
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-h", "inst-h", 5.0, 1_000_000, 0, "CNY"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-h", 1_000_000, "CNY");
    conn.execute("UPDATE accounts SET is_hidden=1 WHERE id='acc-h'", [])
        .unwrap();

    let total = query_investable_assets_cents(&conn).unwrap();
    assert_eq!(total, 0, "隐藏投资账户的现金与持仓一并不进分子");
}

/// 绑定不变式：提取的分子与财务自由度总览的 numerator_cents 在同一现场恒等。
#[test]
fn investable_assets_binds_financial_freedom_numerator() {
    let conn = open();
    seed_account(&conn, "acc-inv", "投资账户", "investment", "CNY", 30_000);
    seed_instrument(&conn, "inst-b", "BIND", "绑定标的", "CNY", "sh");
    seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
    create_transaction_internal(
        &conn,
        make_buy_input_in("acc-inv", "inst-b", 2.0, 1_000_000, 0, "CNY"),
    )
    .unwrap();
    seed_market_price(&conn, "inst-b", 1_500_000, "CNY");

    let extracted = query_investable_assets_cents(&conn).unwrap();
    let overview = query_financial_freedom(&conn).unwrap();
    assert_eq!(
        extracted, overview.numerator_cents,
        "分子提取与自由度总览必须同源等值"
    );
}

/// 折算腿缺汇率让错误上抛（码化 `fx.rate-missing`），不静默返回缺料数字。
#[test]
fn investable_assets_missing_rate_raises_coded_error() {
    let conn = open();
    // 美元投资账户带初始现金：先建余额缓存行（缓存缺失是另一条错误，不属本判据），
    // 再断言现金腿折算缺汇率上抛。
    seed_account(&conn, "acc-usd", "美股账户", "investment", "USD", 80_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();

    let err = query_investable_assets_cents(&conn).unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );
}
