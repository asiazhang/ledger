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

use std::path::Path;

use ledger_infra::db::data_location::{self, DB_FILE_NAME};
use ledger_infra::db::encryption::enable_encryption_for_file;
use ledger_infra::db::{open_connection_in, open_connection_readonly_in, schema_version};
use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{TransactionInput, create_transaction_internal};
use tauri::Manager;
use tauri_app_lib::commands::book;
use tauri_app_lib::commands::boot::BootCell;
use tauri_app_lib::commands::cross_book_summary::cross_book_investment_summary;
use tauri_app_lib::cross_book_summary::{CrossBookBookStatus, CrossBookInvestmentSummary};
use tauri_app_lib::test_support::{
    ScratchDir, isolate_home, seed_account, seed_exchange_rate, seed_instrument, seed_market_price,
};

fn device_app(tag: &str) -> (tauri::AppHandle<tauri::test::MockRuntime>, ScratchDir) {
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let dir = ScratchDir::new(&format!("crossbook-it-{tag}"));
    let app = tauri::test::mock_app();
    let boot = data_location::boot(&dir);
    app.manage(BootCell::new(boot));
    app.manage(ledger_infra::db::open_db_in(&dir).unwrap());
    (app.handle().clone(), dir)
}

fn buy_input(account_id: &str, instrument_id: &str) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "CNY".into(),
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
        quantity: Some(10.0),
        price_cents: Some(1_000_000),
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

fn refresh_balances(conn: &rusqlite::Connection) {
    ledger_accounts::balance::refresh_all_account_balances(conn).unwrap();
}

fn file_bytes(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

fn status_of<'a>(summary: &'a CrossBookInvestmentSummary, name: &str) -> &'a CrossBookBookStatus {
    &summary
        .books
        .iter()
        .find(|b| b.name == name)
        .unwrap_or_else(|| panic!("汇总应含账本 {name}"))
        .status
}

#[tokio::test]
async fn cross_book_summary_command_surface_journey() {
    isolate_home();
    let (handle, dir) = device_app("journey");

    let err = cross_book_investment_summary(handle.clone(), handle.state())
        .await
        .unwrap_err();
    assert!(
        err.is_code("book.registry-unavailable"),
        "注册表不可用应码化拒绝，实际 {err:?}"
    );

    {
        let conn = open_connection_in(&dir).unwrap();
        seed_account(&conn, "acc-inv", "投资账户", "investment", "CNY", 100_000);
        seed_instrument(&conn, "inst-a", "IA", "标的A", "CNY", "sh");
        seed_exchange_rate(&conn, "CNY", "CNY", 1.0);
        create_transaction_internal(&conn, buy_input("acc-inv", "inst-a")).unwrap();
        seed_market_price(&conn, "inst-a", 1_200_000, "CNY");
        refresh_balances(&conn);
    }

    let active_version = {
        let conn = open_connection_readonly_in(&dir).unwrap();
        schema_version(&conn).unwrap()
    };

    let empty = book::create_book(handle.clone(), "空账本".into())
        .await
        .unwrap();
    let usd = book::create_book(handle.clone(), "美股账本".into())
        .await
        .unwrap();
    let locked = book::create_book(handle.clone(), "密码本".into())
        .await
        .unwrap();
    let stale = book::create_book(handle.clone(), "旧版本".into())
        .await
        .unwrap();

    {
        let conn = open_connection_in(&usd.dir).unwrap();
        seed_account(&conn, "acc-usd", "美股现金", "investment", "USD", 10_000);
        conn.execute(
            "INSERT INTO app_settings(key, value) VALUES('ledger.base_currency', '\"USD\"') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .unwrap();
        refresh_balances(&conn);
    }
    {
        let conn = open_connection_in(&locked.dir).unwrap();
        drop(conn);
        enable_encryption_for_file(&locked.dir.join(DB_FILE_NAME), "pw-1234").unwrap();
    }
    {
        let conn = open_connection_in(&stale.dir).unwrap();
        drop(conn);
        let raw = rusqlite::Connection::open(stale.dir.join(DB_FILE_NAME)).unwrap();
        raw.pragma_update(None, "user_version", active_version - 1)
            .unwrap();
    }

    let err = cross_book_investment_summary(handle.clone(), handle.state())
        .await
        .unwrap_err();
    assert!(
        err.is_code("fx.rate-missing"),
        "缺汇率应报 fx.rate-missing，实际 {err:?}"
    );

    {
        let conn = open_connection_in(&dir).unwrap();
        seed_exchange_rate(&conn, "USD", "CNY", 7.0);
    }

    let mut before = std::collections::BTreeMap::new();
    for (name, book_dir) in [
        ("active", dir.path().to_path_buf()),
        ("empty", empty.dir.clone()),
        ("usd", usd.dir.clone()),
        ("locked", locked.dir.clone()),
        ("stale", stale.dir.clone()),
    ] {
        before.insert(name, file_bytes(&book_dir.join(DB_FILE_NAME)));
    }

    let summary = cross_book_investment_summary(handle.clone(), handle.state())
        .await
        .expect("汇总应成功");

    for (name, book_dir) in [
        ("active", dir.path()),
        ("empty", empty.dir.as_path()),
        ("usd", usd.dir.as_path()),
        ("locked", locked.dir.as_path()),
        ("stale", stale.dir.as_path()),
    ] {
        assert_eq!(
            before.get(name).unwrap(),
            &file_bytes(&book_dir.join(DB_FILE_NAME)),
            "账本 {name} 的库文件应零变化"
        );
    }

    assert_eq!(summary.target_currency, "CNY");
    assert!(summary.converted);

    assert_eq!(summary.books.len(), 5);
    assert_eq!(
        status_of(&summary, "默认账本"),
        &CrossBookBookStatus::Included
    );
    assert_eq!(
        status_of(&summary, "空账本"),
        &CrossBookBookStatus::NotInitialized
    );
    assert_eq!(
        status_of(&summary, "美股账本"),
        &CrossBookBookStatus::Included
    );
    assert_eq!(status_of(&summary, "密码本"), &CrossBookBookStatus::Locked);
    assert_eq!(
        status_of(&summary, "旧版本"),
        &CrossBookBookStatus::SchemaMismatch
    );

    assert_eq!(summary.market_value_cents, 120_000);
    assert_eq!(summary.unrealized_pnl_cents, 20_000);
    assert_eq!(summary.cumulative_pnl_cents, 20_000);
    assert_eq!(summary.investable_assets_cents, 190_000);
}
