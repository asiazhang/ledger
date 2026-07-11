pub mod commands;
pub mod db;
pub mod error;
pub mod import_parser;
pub mod models;
pub mod scheduled_transactions;

use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

fn init_database(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let db_state = db::open_db(app.handle())?;
    app.manage(db_state);
    Ok(())
}

fn try_init_database(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    match init_database(app) {
        Ok(()) => Ok(()),
        Err(e) => {
            let confirmed = app
                .dialog()
                .message(format!(
                    "数据库初始化失败：\n\n{e}\n\n是否删除旧数据库后重试？"
                ))
                .title("数据库错误")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::OkCancelCustom(
                    "重置数据库".into(),
                    "退出".into(),
                ))
                .blocking_show();
            if confirmed {
                let dir = app
                    .path()
                    .app_data_dir()
                    .map_err(|e| format!("获取数据目录失败：{e}"))?;
                let db_path = dir.join("ledger.db");
                std::fs::remove_file(&db_path).ok();
                let db_state = db::open_db(app.handle())?;
                app.manage(db_state);
                Ok(())
            } else {
                std::process::exit(0);
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            try_init_database(app).map_err(|e| {
                app.dialog()
                    .message(format!("数据库初始化失败：\n\n{e}"))
                    .title("启动失败")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                std::process::exit(1);
            })
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
