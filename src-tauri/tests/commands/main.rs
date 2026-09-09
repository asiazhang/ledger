//! IPC 命令集成测试入口（账本注册表命令面，issue #833）。
//!
//! 用 tauri::test 的 mock 应用直调命令函数（`#[tokio::test]`），覆盖壳行为：
//! 参数解包、错误码、切换后的引导相位（引导序列内核 `db::boot::plan_boot`）。
//! 壳层业务规则的纯函数语义归内核单测（`db::book_registry` / `db::data_location`），
//! 此处只钉「命令壳 → 内核 → 落盘 → 引导」的整链行为。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
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

mod books;
