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

/// 读取持久化档位（spec #608 / #611）：`app_settings` 的 `logging.level` 键，缺 key
/// 或整表缺失（旧版本备份）回默认 info（`settings::get` 兑底）；解析失败（库内被写入
/// 闭集外字符串）同样回默认 info 并告警——读路径不因坏值上抛，行为免费正确。
/// 界面展示的持久化档位与实际生效档位可能不一致（RUST_LOG 运行时覆盖），属已接受取舍。
pub fn persisted_level(conn: &rusqlite::Connection) -> LogLevel {
    let raw: String = crate::settings::get(
        conn,
        crate::settings::SettingKey::LogLevel,
        "info".to_string(),
    )
    .unwrap_or_else(|e| {
        tracing::warn!(error = %e, "读取持久化日志档位失败，回默认 info");
        "info".to_string()
    });
    raw.parse().unwrap_or_else(|e: String| {
        tracing::warn!(level = %raw, error = %e, "持久化日志档位为闭集外值，回默认 info");
        LogLevel::Info
    })
}

/// 把持久化档位接管到运行期滤镜（spec #608 / #611 启动顺序契约）：启动期 `init`
/// 以「RUST_LOG 环境变量或默认 info」建立滤镜，数据库就绪并读出持久化档位后
/// [`set_level`] 接管一次。显式 `RUST_LOG` 在本次启动内优先（优先级：
/// RUST_LOG > 持久化 > 默认 info），此时不覆盖、只记日志；终端/文件两条输出共用
/// 同一滤镜、一起变化。密文库在解锁换连后再调用（锁定期间库未就绪）。
pub fn apply_persisted_level(conn: &rusqlite::Connection) {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing::info!("RUST_LOG 已显式设置，本次启动内以它为准，跳过持久化档位接管");
        return;
    }
    let level = persisted_level(conn);
    tracing::info!(level = %level.directive(), "按持久化档位接管全局日志滤镜");
    set_level(level);
}

/// 校验闭集 + 持久化 + 运行期接管（`set_log_level` 命令与 BDD 共用，spec #611）：
/// 非闭集档位返回码化错误 `settings.log-level-invalid`（未落库、未接管）；合法档位
/// 写入 `app_settings`（经 settings 模块单点收口、置脏豁免 ADR-0032）后
/// [`set_level`] 接管运行期滤镜。返回解析后的档位供调用方回显。
pub fn set_persisted_level(
    conn: &rusqlite::Connection,
    level_str: &str,
) -> crate::error::Result<LogLevel> {
    let level = level_str.parse::<LogLevel>().map_err(|e: String| {
        crate::error::AppError::codedp(
            "settings.log-level-invalid",
            format!("日志等级非法：{e}"),
            &[level_str],
        )
    })?;
    crate::settings::set(
        conn,
        crate::settings::SettingKey::LogLevel,
        &level.directive(),
    )?;
    set_level(level);
    Ok(level)
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

    // ---- 持久化档位读写（spec #611）----

    fn migrated_conn() -> rusqlite::Connection {
        let mut c = crate::db::open_in_memory().expect("打开内存库");
        crate::db::init_db(&mut c).expect("执行迁移");
        c
    }

    /// 缺 key / 缺表：`persisted_level` 回默认 info（`settings::get` 兑底）。
    #[test]
    fn persisted_level_defaults_to_info_when_unset() {
        assert_eq!(persisted_level(&migrated_conn()), LogLevel::Info);
    }

    /// set→get 回读：`set_persisted_level` 写入后 `persisted_level` 读回一致。
    #[test]
    fn persisted_level_roundtrips_set_level() {
        let conn = migrated_conn();
        set_persisted_level(&conn, "debug").expect("写入 debug 档");
        assert_eq!(persisted_level(&conn), LogLevel::Debug);
    }

    /// 库内残留闭集外字符串：读回兑底默认 info（settings::set 是通用 KV 写、不校验闭集）。
    #[test]
    fn persisted_level_falls_back_when_stored_value_outside_closed_set() {
        let conn = migrated_conn();
        crate::settings::set(&conn, crate::settings::SettingKey::LogLevel, &"verbose")
            .expect("写入闭集外字符串");
        assert_eq!(persisted_level(&conn), LogLevel::Info);
    }

    /// 闭集外档位被拒：未落库、未接管，读回仍默认；错误为码化错误。
    #[test]
    fn set_persisted_level_rejects_outside_closed_set() {
        let conn = migrated_conn();
        let err = set_persisted_level(&conn, "verbose").expect_err("应拒绝闭集外档位");
        assert!(err.is_code("settings.log-level-invalid"));
        assert_eq!(persisted_level(&conn), LogLevel::Info);
    }
}
