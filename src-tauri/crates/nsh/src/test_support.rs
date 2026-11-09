//! NSH 测试器具（#2081 随 api_server 迁居）：API 集成测试（`tests/api_server`）
//! 与壳内单测共用的建库、固定时刻与词法器具面——旧壳同款机制的本壳对应面
//! （根包 test_support 统一工厂链接 tauri/GTK3，与 NSH 的 GTK4 同进程符号
//! 互踩，本 crate 不可消费；#2018 偏离记录先例）。
//!
//! 形态（#2077/#2080 的 facade 纪律归一）：能原文载入的根包文件一律
//! `#[path]` 载入（零复制——词法 scan 与种子闭集随根包原文，规则改动无
//! 第二事实源）；无法载入的（根包 mod.rs 聚合面链接 tauri 面过重）只重建
//! 最小面（[`open`] / [`FIXED_NOW`]，#2077 统一时收拢单点）。
//!
//! 门与可见性（ADR-0111 决策 5 / issue #1132，根包同款）：集成测试链接的是
//! 非 `#[cfg(test)]` 构建的 lib，故本模块以 `cfg(any(test, feature = "test-utils"))`
//! 声明（`lib.rs`），默认不进生产编译；`#[doc(hidden)]` 使其不进入文档。
//! cfg 面涵盖 #2077 的 lib 内嵌测试消费（cfg(test) ⊂ cfg(any(test, test-utils))）。
//!
//! 测试豁免（ADR-0060）：本模块被集成测试以非 cfg(test) 构建链接，无法经
//! crate 根 cfg(test) 豁免覆盖，故文件级放行六件套；生产路径不得消费本模块。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]

/// 夹具簿记戳的统一发放值（ADR-0084 决策 5）：与根包固定时刻登记处
/// （`src-tauri/src/test_support/mod.rs` 的 `FIXED_NOW`，#1468）同值同语义
/// ——迁入测试的簿记戳断言随宿主迁居不改值。守门：check-test-support 规则 3
/// 的登记处唯一在根包（本声明是产品路径顶层项，不进其扫描区域）；本模块
/// `seed` 载入的根包 seed.rs 经 `super::FIXED_NOW` 定位到本值——两侧同值
/// 是载入形态的编译期前提（漂移即种子簿记戳漂移），#2077 统一时退役本副本。
pub const FIXED_NOW: &str = "2026-01-01T00:00:00Z";

/// 内存测试库（统一建库两行序的 infra 快速形态，spec #1086 / issue #1514）：
/// 接线与生产同形（[`crate::wiring::install_domain_seams`]，幂等先装者优先
/// ——余额刷新等写路径接缝未注册即码化错误，建库前必装），建库走 infra
/// 模板还原入口（旧壳 `test_support::open` 的同一 infra 入口）。与根包工厂
/// 的差异面：不接备份置脏 / 写后同步触发 / 期次钩子（NSH 骨架「宿主机制随
/// 宿主落地」裁决，[`crate::wiring`] 刻意不接清单）——迁入集成测试零置脏
/// 断言依赖（#2081 交付报告留痕）。
pub fn open() -> rusqlite::Connection {
    crate::wiring::install_domain_seams();
    ledger_infra::db::open_in_memory_initialized().expect("打开并初始化内存测试库")
}

// #2077/#2080 的 S3 桩 facade 原文载入（scratch + s3 + scan 三件——文件头
// 注释与子载入路径随原文）：scan 是源码扫描守门的词法器具（根包 scan.rs
// 原文，零复制），经下方 re-export 供 api_server 的行情接缝守门与 HTTP
// 半边信号守门消费；ScratchDir 必须住 crate::test_support 根（s3.rs 内
// `use crate::test_support::ScratchDir` 以 crate 根定位，见 s3_stub.rs 头注）。
#[allow(dead_code)]
#[path = "../tests/support/s3_stub.rs"]
mod s3_facade;

pub use s3_facade::{ScratchDir, s3, scan, scratch};

// 迁入集成测试消费的种子闭集：根包 seed.rs 原文载入（零复制——种子 SQL 与
// 根包工厂同源；簿记戳经上方 FIXED_NOW 以 `super::FIXED_NOW` 定位）。只消费
// 子集，facade 放行 dead_code（#2080 同款代价条）。
#[allow(dead_code)]
#[path = "../../../src/test_support/seed.rs"]
pub mod seed;

pub use seed::{
    seed_account, seed_exchange_rate, seed_fx_history_weeks, seed_fx_rate_history, seed_instrument,
    seed_investment_setup,
};
