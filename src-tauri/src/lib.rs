pub mod commands;
pub mod db;
pub mod error;
pub mod import_parser;
pub mod models;
pub mod scheduled_transactions;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let db_state = db::open_db(app.handle())?;
            app.manage(db_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_currencies,
            commands::list_accounts,
            commands::create_account,
            commands::delete_account,
            commands::list_account_balances,
            commands::list_categories,
            commands::create_category,
            commands::delete_category,
            commands::list_transactions,
            commands::create_transaction,
            commands::create_transactions,
            commands::delete_transaction,
            commands::list_exchange_rates,
            commands::create_exchange_rate,
            commands::list_market_prices,
            commands::create_market_price,
            commands::list_instruments,
            commands::create_instrument,
            commands::list_holdings,
            commands::list_budgets,
            commands::create_budget,
            commands::delete_budget,
            commands::monthly_summary,
            commands::category_shares,
            commands::budget_progress,
            commands::preview_import,
            commands::create_scheduled_transaction,
            commands::list_scheduled_transactions,
            commands::get_scheduled_transaction_detail,
            commands::update_scheduled_transaction_status,
            commands::execute_scheduled_occurrence,
            commands::expand_scheduled_occurrences,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
