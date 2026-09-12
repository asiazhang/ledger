// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// 经 crate 根 cfg(test) 整体放行，生产构建零放宽。
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

//! 后端基础设施 crate（spec #1086 / issue #1087）：被所有层消费的能力在此独立
//! 成 crate，依赖方向由 cargo 依赖图强制（壳 → 域 → 基础设施）。定位口径自
//! ADR-0111 决策 1 起为可守门的两条：不依赖任何域 crate；不定义账本数据的
//! 口径与规则。
//!
//! #1088 起本 crate 承载全部基础设施：数据库、错误、设置、文件工具、日志、
//! 事件、信号、闭集与壳层统一读写入口。根包（tauri-app）以再导出形态保留原
//! 引用路径（`crate::db` 等），域与壳层的既有调用点零改动即可编译。
//!
//! crate 内部组织（ADR-0111 决策 2）：`boot/`（引导层，#1131 升顶层）、
//! 原语区（`error` / `fs_util` / `closed_set` / `ids`）、`db/`（库文件与连接
//! 机制）、共享接缝（`events` / `signals` / `settings`）与
//! [`shell_support`]（壳机制暂住——壳层统一读写入口、IPC 载荷
//! 脱敏、日志初始化，只被壳层消费；正住址是壳层，随 #1086 P5 壳层收敛迁出，
//! 不得作为基础设施范式被引用）。失效信号投递机制 `events` 与 `DbState` 的
//! 消费方跨出壳层（备份域、同步域），不随壳机制分组、留在共享接缝。
//!
//! 可见性口径（归位的直接后果）：迁移前以 `pub(crate)` 表达「非公开面」的项，
//! 若消费方在根包（域 / 壳层 / 壳层守门测试），跨 crate 后必须提为 `pub`——
//! #1131 起该口径覆盖面为 `db::tx_scope` 与 `boot::{encryption,
//! passphrase_cache}`（经 db 再导出保持原路径）、`events`、`signals`
//! 少数接缝，签名与语义不变，`grep '^pub(crate)'` 检出的余项仍是本 crate 内部面。

pub mod boot;
pub mod closed_set;
pub mod db;
pub mod error;
pub mod events;
pub mod fs_util;
pub mod ids;
pub mod settings;
pub mod signals;
// 壳机制暂住区（ADR-0111 决策 2 / #1130）：只被壳层消费的机制（读写入口、
// IPC 载荷脱敏、日志初始化）收进显式分组，正住址是壳层（#1086 P5 迁出）；
// crate 根再导出保持原模块路径，既有调用点零改动。
pub mod shell_support;
pub use shell_support::{logger, read_entry, redact, write_entry};
// 测试支持：捕获 tracing 事件的 Layer、全局最大级别稳定器与闸门式假发射器。
// 本模块与基础设施类型（`events::SignalEmitter`）同 crate 是硬约束——经 dev-dependency
// 环消费会让消费方拿到第二份 infra 类型实例（类型身份不相容，issue #1088 实测），
// 故随基础设施归位；根包再导出为 `crate::test_utils` 供集成测试消费。
// 默认不进生产编译（ADR-0111 决策 5 / issue #1132）：仅 `cfg(test)` 与显式启用
// `test-utils` feature 的测试构建可见，生产构建不编译测试器具；根包测试目标经
// dev-dependency 启用 feature（同一编译单元，类型身份不变）。
#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub mod test_utils;
