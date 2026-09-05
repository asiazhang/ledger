use std::path::PathBuf;
use std::str::FromStr;
use std::sync::OnceLock;

use tauri::Manager;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static GUARD: OnceLock<WorkerGuard> = OnceLock::new();
/// 运行期可重载的全局日志滤镜句柄（spec #608 预铸接缝）：
/// `init` 启动期以「RUST_LOG 或默认 info」建立滤镜后登记；`set_level` 经它替换滤镜。
static FILTER_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();

/// 日志等级闭集（spec #608）：error / warn / info / debug / trace。
/// 后端消费、持久化到 `app_settings`，随 Backup/Restore 迁移。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    /// 该档位对应的 EnvFilter 指令，亦是持久化的字符串表示。
    pub fn directive(self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

impl FromStr for LogLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            "trace" => Ok(LogLevel::Trace),
            _ => Err(format!("未知日志档位: `{s}`")),
        }
    }
}

/// 由档位构造对应的全局 EnvFilter（单一全局档位，不做定向 directives）。
pub fn filter_for_level(level: LogLevel) -> EnvFilter {
    EnvFilter::new(level.directive())
}

/// 运行期把全局日志滤镜替换为指定档位（spec #608 预铸接缝，此时无调用方）。
///
/// 行为说明：`init` 启动期仍以「RUST_LOG 环境变量或默认 info」建立滤镜；
/// 本函数只负责把已登记的全局句柄替换为指定档位——后续 `set_log_level` 命令经它切档。
pub fn set_level(level: LogLevel) {
    let Some(handle) = FILTER_HANDLE.get() else {
        // init 尚未登记句柄：没有可切换的全局滤镜，仅记录（区别于启动期 fail loud）。
        tracing::warn!("日志滤镜句柄未初始化，忽略等级切换");
        return;
    };
    if let Err(err) = handle.reload(filter_for_level(level)) {
        tracing::error!(error = %err, "日志等级切换失败");
    }
}

pub fn log_dir() -> &'static PathBuf {
    // B 类豁免（ADR-0060）：日志目录由 init 在启动期首次登记；未初始化即启动装配
    // 缺陷，fail loud。
    #[allow(clippy::expect_used)]
    LOG_DIR.get().expect("logger not initialized")
}

pub fn init(app_handle: &tauri::AppHandle) {
    // B 类豁免（ADR-0060）：日志系统首次初始化失败即无法运行——fail loud。
    #[allow(clippy::expect_used)]
    let dir = app_handle.path().app_log_dir().expect("获取日志目录失败");
    #[allow(clippy::expect_used)]
    std::fs::create_dir_all(&dir).expect("创建日志目录失败");
    LOG_DIR.set(dir.clone()).ok();

    cleanup_old_logs(&dir);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, handle) = reload::Layer::new(filter);

    let (file_writer, guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::daily(&dir, "ledger"));

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
        .with(filter_layer)
        .with(file_layer)
        .with(terminal_layer)
        .init();

    FILTER_HANDLE.set(handle).ok();
    GUARD.set(guard).ok();
}

fn cleanup_old_logs(dir: &PathBuf) {
    let seven_days_ago = chrono::Utc::now() - chrono::Duration::days(7);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.starts_with("ledger.log.")
                && name.len() > 11
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_utils::{CaptureLayer, CapturedEvent, ensure_global_max_level};

    /// 在指定档位滤镜作用下执行 `f`，返回捕获到的 tracing 事件。
    /// 先稳定全局最大级别（`ensure_global_max_level`），使 `debug!`/`trace!` 宏
    /// 快路径不放行级别过滤，真正由该档位的 EnvFilter 决定是否放行（spec #608 接缝 2）。
    fn capture_with_level(level: LogLevel, f: impl FnOnce()) -> Vec<CapturedEvent> {
        ensure_global_max_level();
        let layer = CaptureLayer::new();
        let captured = Arc::clone(&layer.events);
        let subscriber = tracing_subscriber::registry()
            .with(filter_for_level(level))
            .with(layer);
        tracing::subscriber::with_default(subscriber, f);
        captured.lock().unwrap().clone()
    }

    #[test]
    fn debug_level_captures_debug() {
        let events = capture_with_level(LogLevel::Debug, || tracing::debug!("调试信息"));
        assert!(
            events.iter().any(|e| e.level == tracing::Level::DEBUG),
            "debug 档应捕获 debug! 事件，实际: {events:?}"
        );
    }

    #[test]
    fn info_level_suppresses_debug() {
        let events = capture_with_level(LogLevel::Info, || tracing::debug!("不应被记录"));
        assert!(
            !events.iter().any(|e| e.level == tracing::Level::DEBUG),
            "info 档应抑制 debug! 事件，实际: {events:?}"
        );
    }

    #[test]
    fn trace_level_allows_trace() {
        let events = capture_with_level(LogLevel::Trace, || tracing::trace!("追踪信息"));
        assert!(
            events.iter().any(|e| e.level == tracing::Level::TRACE),
            "trace 档应放行 trace! 事件，实际: {events:?}"
        );
    }

    #[test]
    fn reload_layer_switches_filter_at_runtime() {
        // 生产接缝（reload::Layer + Handle）而非裸 EnvFilter：验证运行时重载确实
        // 替换全局滤镜——info 档丢弃 debug!，reload 到 debug 档后同一 callsite 的
        // debug! 被放行（文件/终端两条输出共用同一滤镜的语义依存于此）。
        ensure_global_max_level();
        let (filter_layer, handle) = reload::Layer::new(EnvFilter::new("info"));
        let layer = CaptureLayer::new();
        let captured = Arc::clone(&layer.events);
        let subscriber = tracing_subscriber::registry()
            .with(filter_layer)
            .with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!("info 档：此条应被丢弃");
            handle.reload(filter_for_level(LogLevel::Debug)).unwrap();
            tracing::debug!("debug 档：此条应被捕获");
        });
        let events = captured.lock().unwrap().clone();
        let debug_events: Vec<_> = events
            .iter()
            .filter(|e| e.level == tracing::Level::DEBUG)
            .collect();
        assert_eq!(
            debug_events.len(),
            1,
            "重载到 debug 档后应放行一条 debug! 事件，实际: {events:?}"
        );
        assert!(
            debug_events
                .iter()
                .any(|e| e.fields.iter().any(|(k, _)| k == "message")),
            "被捕获的 debug! 应含 message 字段，实际: {events:?}"
        );
    }

    #[test]
    fn level_directives_round_trip() {
        // 闭集五档由 Directive 往返一致（档位 → 指令字符串 → 档位）。
        for level in [
            LogLevel::Error,
            LogLevel::Warn,
            LogLevel::Info,
            LogLevel::Debug,
            LogLevel::Trace,
        ] {
            assert_eq!(level.directive().parse::<LogLevel>().unwrap(), level);
        }
        // 闭集外字符串应被拒绝。
        assert!("verbose".parse::<LogLevel>().is_err());
    }
}
