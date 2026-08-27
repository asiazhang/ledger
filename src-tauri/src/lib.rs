pub mod api_server;
pub mod auto_backup;
pub mod commands;
pub mod db;
pub mod error;
pub mod events;
pub mod item;
pub mod log_plugin;
pub mod logger;
pub mod models;
pub mod scheduled_transactions;
pub mod settings;
#[doc(hidden)]
pub mod test_utils;
pub mod transaction;

use tauri::Manager;
use tauri::ipc::Invoke;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

pub mod fs_util;

fn init_database(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败：{e}"))?;
    std::fs::create_dir_all(&dir)?;
    // DataLocation 引导（ADR-0018）：建连前先解析库所在目录，必要时启动期搬迁。
    let boot = db::data_location::boot(&dir);
    if let Some(reason) = &boot.fallback_reason {
        tracing::warn!(reason = %reason, "DataLocation 引导发生回退，已改用默认数据目录");
    }
    let db_dir = boot.db_dir.clone();
    // 先登记引导结果再建连：启动失败重置兜底需要知道生效目录。
    app.manage(boot);
    let db_state = db::open_db_in(&db_dir)?;
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
                // 重置兜底作用于 DataLocation 引导解析出的生效目录（引导结果
                // 在建连前已登记；引导本身失败时落到默认数据目录）。
                let db_dir = match app.try_state::<db::data_location::Boot>() {
                    Some(boot) => boot.db_dir.clone(),
                    None => {
                        let dir = app
                            .path()
                            .app_data_dir()
                            .map_err(|e| format!("获取数据目录失败：{e}"))?;
                        std::fs::create_dir_all(&dir)?;
                        dir
                    }
                };
                let db_state = db::reset_db_in(&db_dir)?;
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
            // 传入 AppHandle：参考写入（HTTP 账号/分类 create/delete）成功后
            // emit `ledger:changed`，前端 useReferenceStore 据此自动重拉参考表（issue #79）。
            api_server::start_http_server(
                app.handle().clone(),
                app.state::<db::DbState>().conn.clone(),
            );
            // 后台索引刷新线程：固定周期消费搜索重建队列（ADR-0004 决策 #14，
            // 写路径零索引工作，界面操作不受索引维护影响）。
            commands::search::start_search_refresh_thread(app.state::<db::DbState>().conn.clone());
            // 全量同步中断状态（issue #104）：跨命令共享运行/取消标志。
            app.manage(commands::sync::SyncState::default());
            // 自动备份（issue #125/#126）：目录镜像为进程级单例 [`auto_backup::shared_prefs`]，
            // 轮询调度线程与写路径 on_write 共享同一份；
            // 退出兜底挂在下方 run 事件的 RunEvent::Exit 分支。
            auto_backup::start_scheduler(app.handle());
            // 备份产物变更信号（issue #129）：自动备份的深路径执行点
            // （写时顺带检查 on_write）拿不到 AppHandle，启动时注入镜像句柄一次，
            // 之后经 [`events::emit_backups_changed_current`] 发射。
            events::init_event_app(app.handle());
            Ok(())
        })
        .invoke_handler(logged_invoke_handler(tauri::generate_handler![
            commands::get_ai_prompt,
            commands::create_backup,
            commands::restore_backup,
            commands::restart_app,
            commands::list_backups,
            commands::prune_backups,
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
            commands::search_transactions,
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
            commands::dashboard_overview,
            commands::budget_progress,
            commands::create_scheduled_transaction,
            commands::list_scheduled_transactions,
            commands::get_scheduled_transaction_detail,
            commands::update_scheduled_transaction_status,
            commands::execute_scheduled_occurrence,
            commands::expand_scheduled_occurrences,
            commands::realized_pnl_summary,
            commands::sync_instruments,
            commands::cancel_sync_instruments,
            commands::sync_holding_prices,
            commands::set_auto_backup_dir,
            commands::get_auto_backup_state,
            commands::set_auto_backup_enabled,
        ]))
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // 应用退出兜底（issue #125）：退出前若脏则补一次备份（不受每日约束）。
            if let tauri::RunEvent::Exit = event {
                auto_backup::exit_fallback(app);
            }
        });
}

fn logged_invoke_handler(
    handler: impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<tauri::Wry>| {
        let cmd = invoke.message.command().to_string();
        let payload = invoke.message.payload();
        let id_hint = match payload {
            tauri::ipc::InvokeBody::Json(value) => {
                let id_hint = extract_resource_id(value);
                tracing::info!(command = %cmd, %id_hint, "IPC 调用");
                tracing::debug!(command = %cmd, payload = %value, "IPC 参数");
                id_hint
            }
            _ => {
                tracing::info!(command = %cmd, "IPC 调用");
                String::new()
            }
        };
        // 归因核心：用命令 span 包裹命令执行，使数据库耗时 hook 发射的 SQL 事件
        // 自动继承调用方 span（IPC 命令均为同步函数，与 wrapper 同线程执行，归因成立；
        // 若未来引入异步命令导致丢归因，需对热点函数手包 span 兜底）。
        let span = tracing::info_span!("command", command = %cmd, %id_hint);
        let _entered = span.enter();
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
