//! 域栈日志面（NSH 壳自建：tracing 基座，ADR-0006）：stderr 初始化 + 日志档
//! 位闭集的运行期滤镜接管（spec #608 / #611 / #2053）。
//!
//! 与旧壳 `shell_support::logger` 同语义的 NSH 重建（旧壳在 tauri 根包，本壳
//! 不依赖根包）：档位闭集单点在基建 [`ledger_infra::settings`]（值域常量），
//! 持久化队道单点在 `app_settings.logging.level`（随备份迁移）；
//! - `init`：启动期注册全局滤镜（`RUST_LOG` 优先，缺省 info）并登记可重载句
//!   柄（runtime takeover 的载体）；
//! - `apply_persisted`：库就绪后按持久化档位接管（显式 `RUST_LOG` 在本次启
//!   动内优先、不覆盖只记日志——优先级：RUST_LOG > 持久化 > 默认 info）；
//! - `set_level`：设置卡调档（boot 写路径经基建设置接缝落库后）经句柄换档
//!   ——切档立即生效、跨启动保留；本壳为 stderr 单输出，无文件日志与滚动
//!   面（旧壳日志文件族 NSH 无对应物，见 #2053 偏差记录）。
//!
//! 头less 测试进程内：`init` 已被测试宿主占用 subscriber 时 `try_init` 静默
//! 失败，句柄不登记——`set_level` 走「句柄未初始化」告警路径继续运行（旧壳
//! `set_level` 同语义），持久化层断言不受影响。
//!
//! 测试豁免（ADR-0060）：本模块测试经 crate 根 cfg(test) 整体放行六件套。

use std::sync::OnceLock;

use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// 运行期可重载的全局日志滤镜句柄（spec #608 预铸接缝的 NSH 重建）：`init`
/// 启动期以「RUST_LOG 或默认 info」建立滤镜后登记；`set_level` 经它替换滤镜。
static FILTER_HANDLE: OnceLock<reload::Handle<EnvFilter, tracing_subscriber::Registry>> =
    OnceLock::new();

use tracing_subscriber::reload;

/// 启动期初始化（bin 调用一次）：stderr 输出 + 可重载滤镜（`RUST_LOG` 优先，
/// 缺省 info）。重复初始化（headless 测试宿主已有 subscriber）静默保留现状
/// ——句柄不登记，`set_level` 走告警路径。
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, handle) = reload::Layer::new(filter);
    let layer = tracing_subscriber::fmt::Layer::default()
        .with_ansi(true)
        .with_target(true);
    // 重复初始化不视为失败（bin 注释同语义：嵌入测试宿主重复 init 静默）。
    let _ = tracing_subscriber::registry()
        .with(filter_layer)
        .with(layer)
        .try_init();
    let _ = FILTER_HANDLE.set(handle);
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
