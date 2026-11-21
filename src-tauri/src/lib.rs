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

pub use ledger_accounts;
pub use ledger_backup;
pub use ledger_budget;
pub use ledger_categories;
pub use ledger_currencies;
pub use ledger_dashboard;
pub use ledger_investment;
pub use ledger_item;
pub use ledger_market_sync;
pub use ledger_merchants;
pub use ledger_physical_asset;
pub use ledger_policy;
pub use ledger_reports;
pub use ledger_scheduled;
pub use ledger_sync_engine;
pub use ledger_transaction;
pub mod api_server;
pub mod app_update;
#[cfg(test)]
mod blocking_client_guard;
pub mod commands;
pub mod cross_book_summary;
#[cfg(test)]
mod db_slot_guard;
pub mod shell_support;
#[cfg(test)]
mod signals_cross_check;
#[cfg(test)]
mod sync_trigger_guard;
#[doc(hidden)]
pub mod test_support;
pub mod transaction_wiring;
#[cfg(test)]
mod webkit_render_env_guard;

use crate::shell_support::redact::redact_passphrase_payload;
use ledger_infra::db;
use tauri::Manager;
use tauri::ipc::Invoke;
#[cfg(desktop)]
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::commands::boot::{boot_sequence, recover_boot_failure};
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;

include!(concat!(env!("OUT_DIR"), "/commands_registry.rs"));

const LOCKED_ALLOWED_COMMANDS: &[&str] = &[
    "get_boot_status",
    "get_encryption_status",
    "unlock_encryption",
    "unlock_with_remembered_passphrase",
    "get_remember_passphrase_support",
    "reset_after_forgotten_passphrase",
    "get_backup_meta",
    "restore_backup",
    "restart_app",
];

const BOOT_FAILURE_ALLOWED_COMMANDS: &[&str] = &[
    "get_boot_status",
    "reset_after_startup_failure",
    "get_backup_meta",
    "get_encryption_status",
    "restore_backup",
    "restart_app",
];

pub(crate) fn start_background_services<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    ledger_backup::start_scheduler(app);
    ledger_sync_engine::start_triggers(app);
    ledger_market_sync::start_history_backfill(app);
    ledger_market_sync::start_daily_price_refresh(app);
    ledger_market_sync::start_daily_fx_sync(app);
    crate::app_update::start_auto_update_check(app);
}

fn try_init_database(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("开始初始化数据库");
    match boot_sequence(app) {
        Ok(phase) => {
            tracing::info!(phase = phase.as_str(), "启动引导序列完成");
            Ok(())
        }
        Err(e) => {
            recover_boot_failure(app, &e)?;
            Ok(())
        }
    }
}

#[cfg(not(test))]
fn app_context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

#[cfg(test)]
fn app_context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!(test = true)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(all(target_os = "linux", not(target_os = "android")))]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY（Rust 2024 起 set_var 为 unsafe）：run() 由 main() 同步首调，
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    let gate = EncryptionGate::new(false);
    let ipc_gate = gate.clone();
    let boot_gate = BootFailureGate::new();
    let ipc_boot_gate = boot_gate.clone();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_window_state::Builder::default().build());
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    #[allow(clippy::expect_used)]
    builder
        .setup(move |app| {
            shell_support::logger::init(app.handle());
            ledger_backup::install_after_commit_hook();
            let backup_lock_budget = ledger_backup::LOCK_HOLD_BUDGET;
            ledger_infra::db::register_lock_hold_budget("backup.", backup_lock_budget);
            ledger_infra::db::register_lock_hold_budget("create_backup", backup_lock_budget);
            ledger_infra::db::register_lock_hold_budget("restore_backup", backup_lock_budget);
            ledger_infra::db::register_lock_hold_budget("get_backup_meta", backup_lock_budget);
            ledger_infra::db::register_lock_hold_budget("list_backups", backup_lock_budget);
            ledger_infra::db::register_lock_hold_budget(
                "update.pre-update-backup",
                backup_lock_budget,
            );
            ledger_sync_engine::trigger::install_after_write_hook();
            ledger_accounts::balance::install_balance_refresh_hook();
            ledger_scheduled::install_plan_source_hook();
            ledger_scheduled::auto_run::register_after_occurrence_hook(
                ledger_backup::occurrence_dirty_hook,
            );
            transaction_wiring::install_all();
            ledger_backup::register_catch_up_hook(ledger_scheduled::auto_run::catch_up_hook);
            app.manage(gate.clone());
            app.manage(boot_gate.clone());
            app.manage(app_update::AutoUpdateSwitch::default());
            #[cfg(desktop)]
            app.manage(app_update::DownloadedUpdate::default());
            try_init_database(app.handle()).map_err(|e| {
                #[cfg(desktop)]
                app.dialog()
                    .message(format!("数据库初始化失败：\n\n{e}"))
                    .title("启动失败")
                    .kind(MessageDialogKind::Error)
                    .blocking_show();
                #[cfg(mobile)]
                tracing::error!(error = %e, "数据库初始化失败，带错误退出");
                std::process::exit(1);
            })?;
            let locked = app.state::<EncryptionGate>().is_locked();
            let boot_failed = app.state::<BootFailureGate>().is_failed();
            #[cfg(desktop)]
            api_server::start_http_server(
                app.handle().clone(),
                app.state::<db::DbState>().conn.clone(),
                app.state::<db::DbState>().read_conn.clone(),
                EncryptionGate::clone(&app.state::<EncryptionGate>()),
                BootFailureGate::clone(&app.state::<BootFailureGate>()),
            );
            if !locked && !boot_failed {
                start_background_services(app.handle());
            }
            ledger_infra::events::init_event_app(app.handle());
            Ok(())
        })
        .invoke_handler(logged_invoke_handler(
            ipc_gate,
            ipc_boot_gate,
            tauri_commands_handler(),
        ))
        .build(app_context())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                ledger_backup::exit_fallback(app);
            }
        });
}

fn logged_invoke_handler(
    gate: EncryptionGate,
    boot_gate: BootFailureGate,
    handler: impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static,
) -> impl Fn(Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    move |invoke: Invoke<tauri::Wry>| {
        let cmd = invoke.message.command().to_string();
        if boot_gate.is_failed() && !BOOT_FAILURE_ALLOWED_COMMANDS.contains(&cmd.as_str()) {
            tracing::warn!(command = %cmd, "启动失败期间拒绝 IPC 调用");
            invoke.resolver.reject(db::boot::gate_rejection_error());
            return true;
        }
        if gate.is_locked() && !LOCKED_ALLOWED_COMMANDS.contains(&cmd.as_str()) {
            tracing::warn!(command = %cmd, "应用锁定期间拒绝 IPC 调用");
            invoke.resolver.reject(ledger_infra::error::AppError::coded(
                "encryption.locked",
                "应用已锁定，请先解锁后再操作",
            ));
            return true;
        }
        let payload = invoke.message.payload();
        let id_hint = match payload {
            tauri::ipc::InvokeBody::Json(value) => {
                let id_hint = extract_resource_id(value);
                tracing::info!(command = %cmd, %id_hint, "IPC 调用");
                tracing::debug!(command = %cmd, payload = %redact_passphrase_payload(value), "IPC 参数");
                id_hint
            }
            _ => {
                tracing::info!(command = %cmd, "IPC 调用");
                String::new()
            }
        };
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
