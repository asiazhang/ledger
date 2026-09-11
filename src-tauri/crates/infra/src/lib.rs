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
//! 本票（#1087）只落 workspace 骨架与门禁继承基线的首位成员——IPC 载荷脱敏；
//! 数据库、错误、设置、文件工具、日志、事件、信号、闭集与壳层统一读写入口
//! 随 #1088「基础设施 crate 全量归位」迁入。

pub mod redact;
