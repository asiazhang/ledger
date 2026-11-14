//! NSH 集成测试面共享 S3 桩 facade（issue #2080）：根包 `test_support` 的
//! `s3.rs` / `scratch.rs` / `scan.rs` 子集原文 `#[path]` 载入（立桩时根包
//! 统一测试工厂链接旧壳 tauri/GTK3、与 NSH 的 GTK4 同进程符号互踩，集成面
//! 不可达；#2083 切轨后互踩消失，facade 保留的现由是引用形而非链接约束——
//! `s3.rs` 原文以 `use crate::test_support::ScratchDir` crate 根定位，集成
//! 二进制内须有同名 `test_support` 模块承接；子集只依赖 std + axum + tokio +
//! ledger_infra）。
//!
//! 同步触发接线测试（lib 面 `src/sync_triggers.rs` cfg(test) 消费根包
//! `test_support` 本体，#2083 升根后无需 facade；集成面 `tests/sync_trigger.rs`
//! / `tests/sync_trigger_write.rs` 经本 facade）
//! 用真 HTTP 桩跑真实轮次：断言对准用户可观察结果——上次成功同步时刻落库、
//! 通道 manifest 上出现本机段（旧壳 tests/commands/sync_trigger.rs 同现场形态，
//! 语义在 NSH 侧重生，票面测试决策）。
//!
//! **模块名必须是 `test_support`**：`s3.rs` 内 `use crate::test_support::ScratchDir`
//! 以 crate 根定位facade——集成面载入点：`tests/sync_trigger.rs` 与
//! `tests/sync_trigger_write.rs` 内
//! `#[path = "support/s3_stub.rs"] mod test_support;`。
//!
//! 本文件只随集成测试二进制编译，产品构建不触碰；
//! 结构守门对 tests/ 目录整体豁免（ADR-0056 决策 5）。
//! `scan.rs` 是源码扫描守门的词法器具（掩码与花括号配对原语，std-only），
//! 随本facade一并载入供 NSH 侧扫描守门消费。

/// 集成面只消费根包子集（ScratchDir + spawn_s3_stub/通道读取面；观测/闸门/
/// 故障注入等面归根包 `test_support` 本体的消费方），按 facade 模块放行
/// dead_code——子集消费是共享原文的既定代价（避免为集成面拆分根包文件）。
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
