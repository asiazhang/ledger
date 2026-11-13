//! NSH 两测试面共享的测试 logger 助手（issue #2109）：headless 测试进程缺省无
//! logger 的 opt-in 安装面——OnceLock 单次安装，log facade 记录同步写 stderr
//! 并落入最小捕获快照（[`captured`]），支撑「错误出口被触发」的断言形态。
//!
//! # 坑：headless 缺省无 logger，`log::error!` 静默丢弃（#2089 排查实证，本票起因）
//!
//! NSH headless 组件测试与 lib 单测的进程里**没有任何 logger**：
//! - 壳的真实日志面（`nsh_app::logging::init()`，tracing 基座 stderr + 日滚文件，
//!   ADR-0006）只在产品壳启动时装（`main.rs`），测试进程不走；
//! - NSH 框架只在 `nsh test` 的 runner 二进制装 ForwardLogger（nsh-testing
//!   0.4.0 `runner.rs`），cargo test 的 `Headless` 路径不装；
//! - 后果：被测代码里的 `log::error!`（框架错误出口所在面——nsh-core 组件任务
//!   失败无 error boundary 时的落点、nsh-engine 的 host handler panic、设备
//!   存储写失败等）无处可去，**静默丢弃**。调查「错误出口是否触发」时，logger
//!   缺位 = 真 error 也观察不到（假阴性），#2089 的复现实验因此被迫先自装
//!   logger。任何「headless 下验证错误出口」的排查都应先装本助手。
//!
//! 仓内日志基座是 tracing（ADR-0006），subscriber 同样只在壳启动时装——测试进程里
//! tracing 记录缺省也无处可去。当前依赖图里 tracing 的 log feature 处于启用态
//!（tower → tracing feature "log"，`cargo tree -i log -e features` 实测），tracing
//! 宏会同时经 log facade 发记录，故本助手实测能把仓内 `tracing::warn!/error!`
//! 一并落 stderr 与捕获快照（示范测试 stderr 可见）；这是依赖图性质而非本助手
//! 的承诺面，feature 组合变化时以实测为准。
//!
//! # 用法
//!
//! 测试开头一行 `test_logger::install_test_logger()`：
//! - OnceLock 单次安装，重复调用零副作用（同一进程先到先得）；
//! - ERROR/WARN/INFO 级记录写 stderr（测试失败输出里直接可读），同时落
//!   [`captured`] 快照（断言形态：`captured().iter().any(|l| l.contains(标记))`）；
//! - 级别上限 `LevelFilter::Info` 常量，需更细粒度时改常量（不扩 API 面）。
//!
//! 让位语义：`log::set_logger` 每进程只成交一次，若壳初始化路径已抢先
//!（`logging::init_with` 的 `try_init` 在 tracing-log feature 下会把 LogTracer
//! 装成全局 log logger——产品启动同款形态，log 记录本就经它进 tracing 双输出），
//! 本助手静默让位且捕获面不可用（错误记录仍经既有 logger 可观察）。lib 单测
//! 进程与 `logging` 模块单测共存，槽位归属不确定；headless 组件测试进程无壳
//! 初始化，本助手稳持槽位——进程级接线断言因此住集成面（见下）。
//!
//! 自测分两层：模块内纯单元自测（不经全局槽位，两侧测试二进制都跑）+
//! 集成面进程级接线自测（`app_headless.rs` 的 `install_then_error_is_captured`，
//! 删除 [`install_test_logger`] 内的 `log::set_logger` 接线即红，ADR-0087
//! 负向条目）。
//!
//! 双侧共享同 seed.rs 形态（issue #2027 先例）：lib（`src/tests.rs` 内
//! `#[path = "../tests/support/test_logger.rs"]`）与集成（`tests/app_headless.rs`
//! 内 `#[path = "support/test_logger.rs"]`）各自 `#[path]` 载入，每测试二进制
//! 各得一份副本（独立进程互不串扰）。模块内单元自测在两侧二进制都跑（集成
//! 测试目标恒 `cfg(test)`）。
//!
//! 边界：整理类最小面（#2109 预裁定）——捕获缓冲随进程存活、只增不清，断言用
//! 标记词 `contains` 匹配，不依赖全量精确快照；捕获面只做读取快照，不做清理、
//! 不做级别/目标过滤配置。

// 双侧 `#[path]` 载入、单侧可能不消费全部项（进程级安装助手的消费面住集成
// 面——lib 测试进程与 logging 单测共存，槽位归属不确定，见上「让位语义」；
// lib 侧只载入模块与单元自测），免 lib 侧死码告警。
#![allow(dead_code)]

use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// 级别上限常量（框架错误出口在 ERROR/WARN；Info 保留框架运行态记录，
/// Debug/Trace 会淹没失败输出）。
const MAX_LEVEL: log::LevelFilter = log::LevelFilter::Info;

static INSTALL: OnceLock<()> = OnceLock::new();
static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 为当前测试进程安装测试 logger（opt-in，见模块文档的坑说明）。同一进程
/// 首次调用生效，后续调用零副作用；进程内已有 logger（壳初始化路径的
/// LogTracer）时静默让位（见模块文档「让位语义」）。
pub fn install_test_logger() {
    if INSTALL.set(()).is_ok() {
        let _ = log::set_logger(&TestLogger);
        log::set_max_level(MAX_LEVEL);
    }
}

/// 已捕获日志行快照（`[级别] 目标: 消息` 单行格式；安装生效后的记录才落
/// 捕获，故断言前先 [`install_test_logger`]）。
pub fn captured() -> Vec<String> {
    CAPTURED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= MAX_LEVEL
    }

    fn log(&self, record: &log::Record) {
        let line = format!(
            "[{}] {}: {}",
            record.level(),
            record.target(),
            record.args()
        );
        let _ = writeln!(std::io::stderr(), "{line}");
        CAPTURED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(line);
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod self_tests {
    use super::*;
    use log::Log as _;

    /// 捕获面单元自测（不经全局槽位，两侧测试二进制都跑）：直接调 [`TestLogger`]
    /// 的 `log`，单行格式（级别 + 目标 + 消息）落捕获快照。
    #[test]
    fn log_formats_line_into_capture() {
        let record = log::Record::builder()
            .level(log::Level::Error)
            .target("probe-target-2109")
            .args(format_args!("单元探针消息"))
            .build();
        TestLogger.log(&record);
        assert!(
            captured()
                .iter()
                .any(|l| l == "[ERROR] probe-target-2109: 单元探针消息"),
            "捕获快照应含格式化单行：{:?}",
            captured()
        );
    }

    /// 级别门单元自测：上限 Info——Debug/Trace 不启用，Info/Error/Warn 启用。
    #[test]
    fn enabled_gates_below_info() {
        let enabled_at = |level: log::Level| {
            TestLogger.enabled(
                &log::Metadata::builder()
                    .level(level)
                    .target("probe")
                    .build(),
            )
        };
        assert!(enabled_at(log::Level::Error));
        assert!(enabled_at(log::Level::Warn));
        assert!(enabled_at(log::Level::Info));
        assert!(!enabled_at(log::Level::Debug));
        assert!(!enabled_at(log::Level::Trace));
    }
}
