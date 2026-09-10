//! `commands` 测试目标内**唯一**的 `$HOME` 现场隔离（books / sync_channel /
//! sync_trigger 三个模块共用）。
//!
//! mock runtime 的 `app_data_dir()` 实现为 `dirs::data_dir()` 拼接空 identifier
//!（`$HOME/Library/Application Support`），macOS/Linux 下由 `$HOME` 派生。
//! 重定向 `$HOME` 即可把默认数据目录解析收进进程专属临时区，不触真实用户目录。
//!
//! 收成单点的理由（CI 偶发红根因）：`$HOME` 是进程级全局量，而 cargo 把
//! `tests/commands/` 下所有模块编进**同一个测试二进制**，`#[tokio::test]` 默认
//! 多线程并行。此前 books 与 sync_channel 各持一份 `Once` + 各写一个不同的临时
//! 根，重定向发生两次且值不同：并行时同一用例的两次目录解析（`mock_app()` 取
//! 现场、随后的命令取数据目录）会被对方插在中间，登记目录与引导目录分属两棵
//! 临时树，`plan_boot` 断言随之失败。写入点唯一 + `Once` 唯一后，进程内 `$HOME`
//! 至多变一次、之后再不变，竞态无从发生。

use std::sync::Once;

static ISOLATE: Once = Once::new();

/// HOME 重定向（进程内一次）：调用点在各测试函数首行、现场文件操作之前。
///
/// SAFETY：`set_var` 自 Rust 2024 起 unsafe；本函数是全测试目标唯一的 `$HOME`
/// 写入点，`Once` 保证进程内至多执行一次，写入后该值不再变化。
pub(crate) fn isolate_home() {
    ISOLATE.call_once(|| {
        let root = std::env::temp_dir().join(format!(
            "ledger-commands-it-{}",
            tauri_app_lib::db::new_uuid()
        ));
        std::fs::create_dir_all(&root).unwrap();
        // SAFETY：见函数文档。
        unsafe { std::env::set_var("HOME", &root) };
    });
}
