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

use tauri::Manager;

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use tauri_app_lib::commands::accounts::{create_account, list_accounts};
use tauri_app_lib::commands::boot::restart_app;
use tauri_app_lib::test_support::isolate_home;

type AppHandle = tauri::AppHandle<tauri::test::MockRuntime>;

fn fresh_app() -> AppHandle {
    let app = tauri::test::mock_app();
    let dir = app
        .path()
        .app_data_dir()
        .expect("mock app 应可解析 app_data_dir");
    std::fs::create_dir_all(&dir).unwrap();
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    app.manage(tauri_app_lib::commands::boot::BootCell::new(
        ledger_infra::db::data_location::boot(&dir),
    ));
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    app.handle().clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn first_registration_boot_serves_commands_end_to_end() {
    isolate_home();
    let app = fresh_app();
    restart_app(app.clone()).await.expect("首次登记引导应成功");

    let id = create_account(
        app.state(),
        app.clone(),
        ledger_accounts::AccountInput {
            name: "门面接线账户".into(),
            kind: ledger_accounts::AccountType::Cash,
            currency_code: "CNY".into(),
            initial_balance_cents: Some(12_345),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
        },
    )
    .await
    .expect("写命令应成功");
    let rows = list_accounts(app.state()).await.expect("读命令应成功");
    assert!(
        rows.iter().any(|a| a.id == id && a.name == "门面接线账户"),
        "读命令应读到写命令落库的账户"
    );
}
