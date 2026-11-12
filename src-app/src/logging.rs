//! 域栈日志面（NSH 壳自建：tracing 基座，ADR-0006）：stderr + 日滚文件双输出
//! 与日志档位闭集的运行期滤镜接管（spec #608 / #611 / #2053；文件面随
//! #2077 壳层机制对账补齐）。
//!
//! 与旧壳 `shell_support::logger` 同语义的 NSH 重建（旧壳在 tauri 根包，本壳
//! 不依赖根包）：档位闭集单点在基建 [`ledger_infra::settings`]（值域常量），
//! 持久化队道单点在 `app_settings.logging.level`（随备份迁移）；
//! - `init`：启动期注册全局滤镜（`RUST_LOG` 优先，缺省 info）并登记可重载句
//!   柄（runtime takeover 的载体）；文件面：日志目录 = NSH 侧惯用数据根下的
//!   `logs`（与旧壳 Linux `app_log_dir` 同根同式），日滚（按天）+ 7 天保留清
//!   理与旧壳逐条同语义，`tracing_appender::non_blocking` 的 `WorkerGuard`
//!   进程级持有防丢尾部日志；数据根未注入（headless 测试宿主等嵌入上下文）
//!   则文件面跳过仅 stderr（告警留痕，不 fail loud）；
//! - `apply_persisted`：库就绪后按持久化档位接管（显式 `RUST_LOG` 在本次启
//!   动内优先、不覆盖只记日志——优先级：RUST_LOG > 持久化 > 默认 info）；
//! - `set_level`：设置卡调档（boot 写路径经基建设置接缝落库后）经句柄换档
//!   ——切档立即生效、跨启动保留；终端与文件两条输出共用同一滤镜、一起变化。
//!
//! 头less 测试进程内：`init` 已被测试宿主占用 subscriber 时 `try_init` 静默
//! 失败（滤镜与双输出层都不生效，现状保留）；句柄仍登记但全局订阅者未挂
//! 载——`set_level` 的重载因此静默无观测面（持久化层断言不受影响，旧壳同
//! 容忍）。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// 运行期可重载的全局日志滤镜句柄（spec #608 预铸接缝的 NSH 重建）：`init`
/// 启动期以「RUST_LOG 或默认 info」建立滤镜后登记；`set_level` 经它替换滤镜。
static FILTER_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();

/// 非阻塞文件写手的进程级守卫（旧壳 `GUARD` 同语义）：`tracing_appender` 的
/// 后台写线程在守卫 drop 时冲刷尾部日志——`init` 登记进 `OnceLock` 后进程级
/// 持有，防丢退出前的尾部日志（#2077 平价注意项）。
static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// 日滚文件名前缀：`rolling::daily` 按 `{prefix}.{YYYY-MM-DD}` 产出文件名，
/// 创建（`init`）与清理（[`is_rolling_log_name`]）共用本常量防漂移——旧壳曾
/// 因清理侧前缀写死成 `ledger.log.` 与实际文件名永不相交，7 天保留从未生效
///（#1720 发现），本壳自建起即共用单点。
const LOG_FILE_PREFIX: &str = "ledger";

use tracing_subscriber::reload;

/// 启动期初始化（bin 调用一次）：stderr + 日滚文件双输出 + 可重载滤镜
///（`RUST_LOG` 优先，缺省 info）。重复初始化（headless 测试宿主已有 subscriber）
/// 静默保留现状——句柄不登记，`set_level` 走告警路径。文件面细节见模块头。
pub fn init() {
    init_with(crate::cross_book::default_data_dir(), chrono::Utc::now());
}

/// `init` 的时钟与数据根接缝（私测造证用，生产行为即上一行）：`now` 是清理
/// 保留窗的判定时点（cutoff = now - 7 天，旧壳 [`cleanup_old_logs`] 同式）；
/// `data_root` 缺席 = 文件面跳过仅 stderr（降级不 fail loud，模块头注）。
fn init_with(data_root: Option<PathBuf>, now: chrono::DateTime<chrono::Utc>) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, handle) = reload::Layer::new(filter);

    // 终端面（stderr，与旧壳 terminal_layer 同形）；文件面（日滚 non_blocking）
    // 两条输出共用同一滤镜、一起变化（旧壳同语义）。
    let terminal_layer = tracing_subscriber::fmt::Layer::default()
        .with_ansi(true)
        .with_target(true)
        .with_line_number(true);
    let file_layer = data_root.map(|root| {
        let dir = root.join("logs");
        // 目录创建由 appender 承担（tracing-appender create_writer 内建
        // create_dir_all，失败即 panic——旧壳显式 create_dir_all + expect 同果
        // 同 fail-loud 口径）；清理先于落盘，首启无目录时无产物可清，不阻。
        cleanup_old_logs(&dir, now - chrono::Duration::days(7));
        let (file_writer, guard) =
            tracing_appender::non_blocking(tracing_appender::rolling::daily(&dir, LOG_FILE_PREFIX));
        let _ = GUARD.set(guard);
        tracing_subscriber::fmt::Layer::default()
            .with_writer(file_writer)
            .with_ansi(false)
            .with_target(true)
            .with_line_number(true)
    });

    // 重复初始化不视为失败（bin 注释同语义：嵌入测试宿主重复 init 静默）。
    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(file_layer)
        .with(terminal_layer)
        .try_init();
    let _ = FILTER_HANDLE.set(handle);
}

/// 滚动清理（按天滚动、保留 7 天，issue #283 的旧壳同语义重建）：删除目录内
/// 命中本壳日滚命名且时点早于 `cutoff` 的文件；`cutoff` 由 [`init_with`] 以
/// 「now - 7 天」传入。命中文件的时点取 `created`（优先）失败时回退
/// `modified`（旧壳逐条同式）；删除失败静默忽略（旧壳同口径，不因清理阻断
/// 启动）。
fn cleanup_old_logs(dir: &Path, cutoff: chrono::DateTime<chrono::Utc>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && is_rolling_log_name(name)
                && let Ok(metadata) = std::fs::metadata(&path)
                && let Ok(modified) = metadata.created().or_else(|_| metadata.modified())
            {
                let datetime: chrono::DateTime<chrono::Utc> = modified.into();
                if datetime < cutoff {
                    std::fs::remove_file(&path).ok();
                }
            }
        }
    }
}

/// 判断是否本壳日滚产物 `{LOG_FILE_PREFIX}.{YYYY-MM-DD}`；日期须可解析，
/// 目录内无关文件不命中（旧壳同式）。
fn is_rolling_log_name(name: &str) -> bool {
    name.strip_prefix(LOG_FILE_PREFIX)
        .and_then(|rest| rest.strip_prefix('.'))
        .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        .is_some()
}

/// 由档位构造对应的全局 EnvFilter（单一全局档位，不做定向 directives）。
pub fn filter_for_level(level: LogLevel) -> EnvFilter {
    EnvFilter::new(level.directive())
}

/// 运行期把全局日志滤镜替换为指定档位（spec #608 / #611）：boot 写路径落库
/// 成功后调用，切档立即生效。句柄未登记（init 被测试宿主抢先）时仅记录告警
/// ——档位不变属已接受边界（持久化层照常生效，下次启动接管）。
pub fn set_level(level: LogLevel) {
    let Some(handle) = FILTER_HANDLE.get() else {
        tracing::warn!("日志滤镜句柄未初始化，本次会话内档位保持现状");
        return;
    };
    if let Err(err) = handle.reload(filter_for_level(level)) {
        tracing::error!(error = %err, "日志等级切换失败");
    }
}

/// 读取持久化档位（spec #608 / #611）：`app_settings` 的 `logging.level` 键，
/// 缺 key / 缺表回默认 info；闭集外值回默认 info 并告警（读路径不因坏值上
/// 抛，旧壳 `persisted_level` 同口径）。入参 = 会话句柄（读连接侧取锁内的读
/// 面由引用折叠承接，本模块不点名 rusqlite——依赖面归基建与调用方）。
pub fn persisted_level(db: &ledger_infra::db::DbState) -> String {
    let raw: Option<String> = match db.read_conn.lock() {
        Ok(guard) => ledger_infra::settings::get(
            &guard,
            ledger_infra::settings::SettingKey::LogLevel,
            "info".to_string(),
        )
        .ok(),
        Err(e) => {
            tracing::warn!(error = %e, "读取持久化日志档位失败（连接锁不可用），回默认 info");
            None
        }
    };
    match raw {
        Some(raw) if ledger_infra::settings::LOG_LEVEL_DIRECTIVES.contains(&raw.as_str()) => raw,
        Some(raw) => {
            tracing::warn!(level = %raw, "持久化日志档位为闭集外值，回默认 info");
            "info".to_owned()
        }
        None => "info".to_owned(),
    }
}

/// 日志档位闭集枚举（运行期滤镜切换的入参形态；档位指令住闭集常量）。旧壳
/// `logger::LogLevel` 的 NSH 对应物——本壳不为它立旁路持久化结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// 只记错误。
    Error,
    /// 错误与告警。
    Warn,
    /// 错误、告警与常规信息。
    Info,
    /// 追加调试信息。
    Debug,
    /// 全部追踪。
    Trace,
}

impl LogLevel {
    /// 该档位对应的 EnvFilter 指令（基建闭集常量的下标单点投影）。
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

/// 校验闭集外指令回 None（boot 读路径与启动接管的分布收口）。
pub fn parse_directive(raw: &str) -> Option<LogLevel> {
    match raw {
        "error" => Some(LogLevel::Error),
        "warn" => Some(LogLevel::Warn),
        "info" => Some(LogLevel::Info),
        "debug" => Some(LogLevel::Debug),
        "trace" => Some(LogLevel::Trace),
        _ => None,
    }
}

/// 启动接管（旧壳 `apply_persisted_level` 同语义）：显式 `RUST_LOG` 在本次启
/// 动内优先（不覆盖、只记日志）；否则读持久化档位并接管滤镜。启动初段按
/// 「环境变量或默认档」记录的短窗口属固有序约束。入参 = 会话句柄（run_startup
/// 就绪路径的直接消费面）。
pub fn apply_persisted(db: &ledger_infra::db::DbState) {
    if std::env::var_os("RUST_LOG").is_some() {
        tracing::info!("RUST_LOG 已显式设置，本次启动内以它为准，跳过持久化档位接管");
        return;
    }
    let raw = persisted_level(db);
    tracing::info!(level = %raw, "按持久化档位接管全局日志滤镜");
    if let Some(level) = parse_directive(&raw) {
        set_level(level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    /// 唯一暂存数据根（测试暂存目录器具发放，RAII guard 整树清理，#2112）：
    /// 「进程退出由 OS 回收」是错误假设，残留随用例数堆积。
    fn temp_root(tag: &str) -> crate::test_support::ScratchDir {
        crate::test_support::ScratchDir::new(&format!("nsh-logs-{tag}"))
    }

    /// 在日志目录内预置一个文件（调用方保证父目录已建）。
    fn plant(logs: &std::path::Path, name: &str) {
        File::create(logs.join(name)).unwrap();
    }

    /// `rolling::daily(LOG_FILE_PREFIX)` 的真实产物名必须命中清理匹配：前缀
    /// 再次写死成别的值即红（旧壳 #1720 缺陷的负向条目同款，清理与创建共
    /// 用本前缀常量防漂移）。
    #[test]
    fn rolling_log_names_match_cleanup() {
        assert!(is_rolling_log_name("ledger.2026-08-28"));
        assert!(is_rolling_log_name(&format!(
            "{LOG_FILE_PREFIX}.2026-01-01"
        )));
    }

    /// 纯前缀、日期不可解析、异前缀的目录内无关文件不得被清理命中。
    #[test]
    fn cleanup_skips_unrelated_names() {
        assert!(!is_rolling_log_name("ledger"));
        assert!(!is_rolling_log_name("ledger.README"));
        assert!(!is_rolling_log_name("other.2026-08-28"));
        assert!(!is_rolling_log_name("ledger.log.2026-08-28"));
    }

    /// 文件面接线 · 目录建立（#2077 平价，删除接线即红）：数据根注入后 `init`
    /// 在数据根下建出 `logs` 目录（本用例不预建目录，目录只能来自文件面接线
    /// ——删除 `file_layer` 块即红）。
    #[test]
    fn init_creates_log_dir_under_data_root() {
        let root = temp_root("mklogdir");
        let logs = root.join("logs");
        assert!(!logs.exists(), "前置：测试不预建 logs 目录");
        init_with(Some(root.to_path_buf()), chrono::Utc::now());
        assert!(logs.is_dir(), "init 应在数据根下建 logs 目录");
    }

    /// 文件面接线 · 滚动清理（#2077 平价，删除接线即红）：预置过期日滚产物
    /// 后 `init` 清理超出保留窗的文件——删 `cleanup_old_logs` 调用（过期文件
    /// 残留）即红。过期判定经时钟接缝造证（now 前移 8 天 ⇒ 清理线越过预置
    /// 文件的时点），不依赖文件系统 btime 的可造性（created 优先的旧壳语义
    /// 原样保留）。
    #[test]
    fn init_removes_rolling_files_beyond_retention() {
        let root = temp_root("init");
        let logs = root.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        plant(&logs, "ledger.2020-01-01");
        init_with(
            Some(root.to_path_buf()),
            chrono::Utc::now() + chrono::Duration::days(8),
        );
        assert!(
            !logs.join("ledger.2020-01-01").exists(),
            "超出保留窗的日滚产物应被清理"
        );
    }

    /// 保留窗内产物与目录内无关文件不清理：真实时钟档（cutoff = now-7d）下
    /// 预置文件的新鲜时点不越线，原样保留；清理只命中本壳日滚命名。
    #[test]
    fn init_keeps_fresh_rolling_files_and_unrelated_names() {
        let root = temp_root("keep");
        let logs = root.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let today = format!(
            "{LOG_FILE_PREFIX}.{}",
            chrono::Utc::now().format("%Y-%m-%d")
        );
        plant(&logs, &today);
        plant(&logs, "unrelated.txt");
        init_with(Some(root.to_path_buf()), chrono::Utc::now());
        assert!(logs.join(&today).exists(), "保留窗内日滚产物不清理");
        assert!(logs.join("unrelated.txt").exists(), "无关文件不清理");
    }

    /// 数据根未注入（headless 测试宿主等嵌入上下文）：文件面跳过、仅 stderr
    /// 的降级路径不 fail loud——改为缺根即 panic 的实现使本用例变红。
    #[test]
    fn init_without_data_root_degrades_to_stderr_only() {
        init_with(None, chrono::Utc::now());
    }
}
