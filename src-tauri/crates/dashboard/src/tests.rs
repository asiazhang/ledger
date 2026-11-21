use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};

use crate::DashboardOverview;
use crate::net_worth::{current_fingerprint, write as write_cache};
use crate::query_dashboard_overview;
use tauri_app_lib::test_support::snapshot_probe::{self, InjectionOutcome};
use tauri_app_lib::test_support::{
    ScratchDir, open, open_file, seed_account, seed_exchange_rate, seed_fx_history_weeks,
    seed_instrument, seed_market_price,
};

fn make_buy_input(account_id: &str, instrument_id: &str, qty: f64, price: i64) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
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
        price_cents: Some(price),
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
    seed_market_price(&conn, "inst-dash", 100_000, "USD");

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
        after.accounts_balance_cents, 930_000,
        "余额腿（现金户 1000000 + 证券户现金 −70000）必须与种子快照同时点（注入写落在余额腿与持仓腿之间即读到旧值）"
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
        after.net_worth_cents, 1_000_000,
        "总量必须等于三腿和且与种子快照同时点（腿间写提交即产出现金未扣 + 仓位翻倍的双计数，撕裂值 1070000 不属于任何快照）"
    );
    assert_eq!(after.native_currency, "CNY", "折算基准币种不应漂移");
}

#[test]
fn legacy_formula_cache_row_is_recomputed() {
    let dir = ScratchDir::new("dashboard-overview-legacy-cache");
    let conn = open_file(dir.path());
    seed_account(&conn, "acc-legacy-cash", "现金户", "cash", "CNY", 500_000);
    seed_account(
        &conn,
        "acc-legacy-inv",
        "证券户",
        "investment",
        "CNY",
        200_000,
    );
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();

    let current = current_fingerprint(&conn).unwrap();
    let (_tag, legacy_fingerprint) = current.split_once('|').unwrap();
    write_cache(
        &conn,
        legacy_fingerprint,
        &DashboardOverview {
            native_currency: "CNY".into(),
            net_worth_cents: 500_000,
            accounts_balance_cents: 500_000,
            holdings_market_value_cents: 0,
            physical_assets_value_cents: 0,
        },
    )
    .unwrap();

    let overview = query_dashboard_overview(&conn).unwrap();
    assert_eq!(
        overview.net_worth_cents, 700_000,
        "旧口径缓存行不得被读出：必须按当前口径（现金户 + 证券户现金）重算"
    );
    assert_eq!(overview.accounts_balance_cents, 700_000);
}

#[test]
fn dashboard_overview_net_worth_includes_physical_assets_leg() {
    let conn = open();
    seed_account(&conn, "acc-phys-cash", "现金户", "cash", "CNY", 300_000);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();
    ledger_physical_asset::create_physical_asset(
        &conn,
        ledger_physical_asset::PhysicalAssetInput {
            name: "金条".into(),
            purchase_date: None,
            purchase_price_cents: None,
            purchase_currency_code: None,
            initial_valuation_cents: Some(200_000),
            initial_valuation_currency_code: Some("CNY".into()),
            initial_valuation_date: Some("2026-01-10".into()),
        },
        &mut || {},
    )
    .unwrap();

    let overview = query_dashboard_overview(&conn).unwrap();
    assert_eq!(overview.accounts_balance_cents, 300_000);
    assert_eq!(overview.holdings_market_value_cents, 0);
    assert_eq!(
        overview.physical_assets_value_cents, 200_000,
        "实物腿按在持估值（native 币种）计入"
    );
    assert_eq!(
        overview.net_worth_cents, 500_000,
        "总量 = 三腿之和（余额 + 持仓 + 实物）"
    );
}
