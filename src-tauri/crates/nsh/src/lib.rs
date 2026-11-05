// 测试整体豁免（ADR-0060）：clippy 六件套 deny 仅约束生产路径；单元测试目标
// （含 src/** 内 #[cfg(test)] 模块与外挂 tests.rs）经 crate 根 cfg(test) 整体
// 放行，生产构建零放宽。
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

//! NSH 原生壳骨架（issue #2017 / ADR-0145 拆票①首块桩，spec #2015）。
//!
//! 双壳并存期的第一块桩：以 NSH（Native Shell）Rust 模式承载的最小壳竖井——
//! 启动参数给账本目录，引导域栈打开**未加密**账本库（无解锁向导），窗口显示
//! 真实库的交易总数。读路径直调域服务（`ledger-transaction` 读路径），不经任何
//! IPC 命令边界——ADR-0145 决策 4 的接缝形态立在此处；Tauri 旧壳零改动、保持
//! 唯一发版产品。
//!
//! 依赖方向沿用壳 → 域 → 基础设施 → 协议（ADR-0112）：本 crate 是壳层 bin
//! crate（结构守门政策核 CRATES 登记在册），只消费域 crate 与基础设施，不定义
//! 账本语义；金额与折算口径以核心交易域为唯一真源，本壳只消费不另立。
//!
//! 模块地图：
//! - [`app`]：NSH 根组件与启动结果视图模型（装配入口可注入，headless 可测）；
//! - [`boot`]：域栈引导——库文件处置判定（复用基础设施引导层）、成对建连与
//!   交易总数读取；
//! - [`wiring`]：域接缝接线——读路径全部读时投影接缝 + 交易写入协议的余额刷新
//!   接缝，与旧壳启动接线同形（ADR-0112 决策 5）。
//!
//! 测试权威（ADR-0087 断言归属在本壳的落点）：壳行为（引导与读接线）归本
//! crate 的 lib 单测；视图树接线（窗口呈现）归 headless 组件测试
//! （`tests/app_headless.rs`，docs/research/nsh-testing-capabilities.md §3.1）；
//! 域语义归各域单测，不在本壳重复。

pub mod app;
pub mod boot;
pub mod wiring;

#[cfg(test)]
mod tests;
