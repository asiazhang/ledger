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

//! 后端基础设施 crate（spec #1086 / issue #1087）：无域语义、被所有层消费的
//! 能力在此独立成 crate，依赖方向由 cargo 依赖图强制（壳 → 域 → 基础设施）。
//!
//! #1088 起本 crate 承载全部基础设施：数据库、错误、设置、文件工具、日志、
//! 事件、信号、闭集与壳层统一读写入口。根包（tauri-app）以再导出形态保留原
//! 引用路径（`crate::db` 等），域与壳层的既有调用点零改动即可编译。
//!
//! 可见性口径（归位的直接后果）：迁移前以 `pub(crate)` 表达「非公开面」的项，
//! 若消费方在根包（域 / 壳层 / 壳层守门测试），跨 crate 后必须提为 `pub`——
//! 全部在 `db::{tx_scope, encryption, passphrase_cache}`、`events`、`signals`
//! 少数接缝上，签名与语义不变，`grep '^pub(crate)'` 检出的余项仍是本 crate 内部面。

pub mod closed_set;
pub mod db;
pub mod error;
pub mod events;
pub mod fs_util;
pub mod ids;
pub mod logger;
pub mod read_entry;
pub mod redact;
pub mod settings;
pub mod signals;
// 测试支持：捕获 tracing 事件的 Layer、全局最大级别稳定器与闸门式假发射器。
// 本模块与基础设施类型（`events::SignalEmitter`）同 crate 是硬约束——经 dev-dependency
// 环消费会让消费方拿到第二份 infra 类型实例（类型身份不相容，issue #1088 实测），
// 故随基础设施归位；根包再导出为 `crate::test_utils` 供集成测试消费。
#[doc(hidden)]
pub mod test_utils;
pub mod write_entry;
