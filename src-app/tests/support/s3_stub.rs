//! NSH 测试面共享 S3 桩（issue #2080）：根包 `test_support` 的 tauri-free 子集（#2083 切轨后即根包本体）
//! 原文 `#[path]` 载入（纪律同 `seed.rs`——根包统一测试工厂链接 tauri/GTK3，
//! 与 NSH 的 GTK4 同进程符号互踩不可达；`s3.rs` / `scratch.rs` / `scan.rs`
//! 只依赖 std + axum + tokio + ledger_infra，NSH 测试构建无符号互踩）。
//!
//! 同步触发接线测试（`src/sync_triggers.rs` cfg(test) 与 `tests/app_headless.rs`）
//! 用真 HTTP 桩跑真实轮次：断言对准用户可观察结果——上次成功同步时刻落库、
//! 通道 manifest 上出现本机段（旧壳 tests/commands/sync_trigger.rs 同现场形态，
//! 语义在 NSH 侧重生，票面测试决策）。
//!
//! **模块名必须是 `test_support`**：`s3.rs` 内 `use crate::test_support::ScratchDir`
//! 以 crate 根定位facade——两侧载入点：
//! - lib：`lib.rs` 内 `#[cfg(test)] #[path = "tests/support/s3_stub.rs"] mod test_support;`
//! - 集成：`tests/app_headless.rs` 内 `#[path = "support/s3_stub.rs"] mod test_support;`
//!
//! 本文件只随两侧测试目标编译（cfg(test) / 测试二进制），产品构建不触碰；
//! 结构守门对 tests/ 目录整体豁免（ADR-0056 决策 5）。
//! `scan.rs` 是源码扫描守门的词法器具（掩码与花括号配对原语，std-only），
//! 随本facade一并载入供 NSH 侧扫描守门消费。

/// NSH 侧只消费根包子集（ScratchDir + spawn_s3_stub/通道读取面；观测/闸门/
/// 故障注入等面归旧壳测试消费），按 facade 模块放行 dead_code——子集消费是
/// 共享原文的既定代价（避免为 NSH 拆分根包文件）。
#[allow(dead_code)]
#[path = "../../src/test_support/scratch.rs"]
pub mod scratch;

#[allow(dead_code)]
#[path = "../../src/test_support/s3.rs"]
pub mod s3;

#[allow(dead_code)]
#[path = "../../src/test_support/scan.rs"]
pub mod scan;

pub use scratch::ScratchDir;
