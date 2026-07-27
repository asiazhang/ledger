use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn log_dir() -> &'static PathBuf {
    LOG_DIR.get().expect("logger not initialized")
}

pub fn init(app_handle: &tauri::AppHandle) {
    let dir = app_handle
        .path()
        .app_log_dir()
        .expect("获取日志目录失败");
    std::fs::create_dir_all(&dir).expect("创建日志目录失败");
    LOG_DIR.set(dir.clone()).ok();

    cleanup_old_logs(&dir);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let (file_writer, guard) = tracing_appender::non_blocking(
        tracing_appender::rolling::daily(&dir, "ledger"),
    );

    let file_layer = Layer::default()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(true)
        .with_line_number(true);

    let terminal_layer = Layer::default()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(true)
        .with_line_number(true);

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(terminal_layer)
        .init();

    GUARD.set(guard).ok();
}

fn cleanup_old_logs(dir: &PathBuf) {
    let seven_days_ago = chrono::Utc::now() - chrono::Duration::days(7);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.starts_with("ledger.log.") && name.len() > 11
                && let Ok(metadata) = std::fs::metadata(&path)
                && let Ok(modified) = metadata.created().or_else(|_| metadata.modified())
            {
                let datetime: chrono::DateTime<chrono::Utc> = modified.into();
                if datetime < seven_days_ago {
                    std::fs::remove_file(&path).ok();
                }
            }
        }
    }
}
