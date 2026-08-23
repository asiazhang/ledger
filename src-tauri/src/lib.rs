pub mod api_server;
pub mod commands;
pub mod db;
pub mod error;
pub mod log_plugin;
pub mod logger;
pub mod models;
pub mod scheduled_transactions;

use tauri::Manager;
use tauri::ipc::Invoke;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

fn init_database(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let db_state = db::open_db(app.handle())?;
    app.manage(db_state);
    tracing::info!("数据库初始化完成");
    Ok(())
}

fn try_init_database(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("开始初始化数据库");
    match init_database(app) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::error!(error = %e, "数据库初始化失败");
            let confirmed = app
                .dialog()
                .message(format!(
                    "数据库初始化失败：\n\n{e}\n\n是否备份旧数据并重置数据库？"
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
                let bak_path = db_path.with_extension("db.bak");
                std::fs::rename(&db_path, &bak_path).ok();
                tracing::info!("已备份原数据库并重置");
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
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(log_plugin::init())
        .setup(|app| {
            logger::init(app.handle());
            try_init_database(app).map_err(|e| {
                app.dialog()
                    .message(format!("数据库初始化失败：\n\n{e}"))
                    .title("启动失败")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                std::process::exit(1);
            })?;
            api_server::start_http_server(app.state::<db::DbState>().conn.clone());
            Ok(())
        })
        .invoke_handler(logged_invoke_handler(tauri::generate_handler![
            commands::get_ai_prompt,
            commands::create_backup,
            commands::restore_backup,
            commands::restart_app,
            commands::list_currencies,
            commands::list_accounts,
            commands::create_account,
            commands::delete_account,
            commands::list_account_balances,
            commands::list_categories,
            commands::create_category,
            commands::update_category,
            commands::reorder_categories,
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
            commands::create_scheduled_transaction,
            commands::list_scheduled_transactions,
            commands::get_scheduled_transaction_detail,
            commands::update_scheduled_transaction_status,
            commands::execute_scheduled_occurrence,
            commands::expand_scheduled_occurrences,
            commands::realized_pnl_summary,
            commands::sync_instruments,
        ]))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn logged_invoke_handler(
    handler: impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<tauri::Wry>| {
        let cmd = invoke.message.command().to_string();
        let payload = invoke.message.payload();
        match payload {
            tauri::ipc::InvokeBody::Json(value) => {
                let id_hint = extract_resource_id(value);
                tracing::info!(command = %cmd, %id_hint, "IPC 调用");
                tracing::debug!(command = %cmd, payload = %value, "IPC 参数");
            }
            _ => {
                tracing::info!(command = %cmd, "IPC 调用");
            }
        }
        handler(invoke)
    }
}

fn extract_resource_id(payload: &serde_json::Value) -> String {
    if let serde_json::Value::Object(map) = payload {
        for key in ["id", "account_id", "category_id", "occurrence_id"] {
            if let Some(val) = map.get(key) {
                if let Some(s) = val.as_str() {
                    if s.len() > 8 {
                        return format!("{}…", &s[..8]);
                    }
                    return s.to_string();
                }
                if let Some(n) = val.as_i64() {
                    return n.to_string();
                }
            }
        }
        if let Some(input) = map.get("input") {
            return extract_resource_id(input);
        }
    }
    String::new()
}
