//! 仪表盘域单测（issue #1702）：多语句读闭包的快照一致性探针——净资产三腿
//! （余额 / 持仓 / 实物）必须同快照。探针机制见
//! `tauri_app_lib::test_support::snapshot_probe`；域内测试目标随本票立项，
//! 其余纯读聚合仍由根包侧三层测试覆盖（壳层命令集成与 e2e BDD）。

use rusqlite::Connection;

use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};

use crate::query_dashboard_overview;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{
    ScratchDir, open_file, seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_instrument,
};

/// 种入标的当前行情（现价缓存单行，`v_holdings` 据此算持仓腿市值）。现价是
/// market_prices 单行，不在种子工厂登记处，按既有投资域测试先例裸插。
fn seed_market_price(conn: &Connection, instrument_id: &str, price_cents: i64, currency: &str) {
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

/// 买入输入（价格权威形态：金额 = 数量 × 单价 + 手续费；先例：投资域测试
/// common 的同名构造器，域特有形态不上收工厂）。
fn make_buy_input(account_id: &str, instrument_id: &str, qty: f64, price: i64) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
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
        price_cents: Some(price),
        fee_cents: Some(0),
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
    }
}

/// 净资产三腿（余额 / 持仓 / 实物）必须同快照（issue #1702）：总览由余额腿
/// （accounts × balance cache）、持仓腿（v_holdings）、实物腿多段语句拼出，
/// 「总量 = 三腿和」要求三腿同时点。探针在持仓腿读取开始前于另一连接**原子**提交
/// 「现金缓存减 50000 + 现价翻倍」——
/// - 读闭包无快照保护（红）：余额腿读旧（1000000）、持仓腿读新（140000），
///   总量 1140000 不属于任何快照（前快照 1070000、后快照 1120000）——
///   「现金已挪动而余额腿未见」的双计形态；
/// - 读闭包收进读事务（绿）：注入写被挡住，三腿同见种子一套数。
#[test]
fn dashboard_overview_legs_share_one_snapshot() {
    let dir = ScratchDir::new("dashboard-overview-read-snapshot");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-dash-cash", "现金户", "cash", "CNY", 1_000_000);
    seed_account(&conn, "acc-dash-inv", "证券户", "investment", "USD", 0);
    seed_fx_history_weeks(&conn, "USD", "CNY", 7.0, &["2026-01-10"]);
    seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    seed_instrument(&conn, "inst-dash", "AAPL", "Apple", "USD", "unknown");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-dash-inv", "inst-dash", 10.0, 100_000),
    )
    .unwrap();
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    // 现价 = 买入价（10 元/股）：持仓腿 = 10 股 × 10000 分 × 7 = 70000 分（CNY）。
    seed_market_price(&conn, "inst-dash", 100_000, "USD");

    // 探针：持仓腿读取（`FROM v_holdings h JOIN accounts a`，与余额腿的
    // accounts / account_balance_cache 读取区分）开始前，另一连接提交两语句写。
    snapshot_probe::arm(
        &conn,
        dir.path(),
        "FROM v_holdings h JOIN accounts a",
        &[
            "UPDATE account_balance_cache SET balance_cents = balance_cents - 50000 \
             WHERE account_id = 'acc-dash-cash'",
            "UPDATE market_prices SET price_cents = price_cents * 2",
        ],
    );
    let after = query_dashboard_overview(&conn).unwrap();

    let outcome = snapshot_probe::outcome();
    assert!(
        outcome != InjectionOutcome::NotFired,
        "探针未命中持仓腿读取（marker 漂移或未臂装），断言失去意义：{outcome:?}"
    );

    assert_eq!(
        after.accounts_balance_cents, 1_000_000,
        "余额腿必须与种子快照同时点（注入写落在余额腿与持仓腿之间即读到旧值）"
    );
    assert_eq!(
        after.holdings_market_value_cents, 70_000,
        "持仓腿必须与种子快照同时点"
    );
    assert_eq!(
        after.physical_assets_value_cents, 0,
        "无实物资产时第三腿为零（否则种子口径漂移）"
    );
    assert_eq!(
        after.net_worth_cents, 1_070_000,
        "总量必须等于三腿和（同快照；腿间写提交即产出现金未扣 + 仓位翻倍的双计数）"
    );
    assert_eq!(after.native_currency, "CNY", "折算基准币种不应漂移");
}
