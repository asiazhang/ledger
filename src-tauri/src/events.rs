//! 事件发射（issue #79）：参考写入成功后的 `ledger:changed` 失效信号。
//!
//! `ledger:changed` 是**通用、粗粒度、无 payload** 的信号：参考数据
//! （`currencies / accounts / categories`）任一写入成功后由调用方 emit，
//! 前端 `useReferenceStore` 订阅后自动重拉三张参考表。交易类写入本期**不触发**
//! （不改参考表），日后需要交易视图实时刷新时再补 emitter，不重设计信号。
//!
//! 「是否为参考写入」的判定收敛在本模块：IPC 命令清单见
//! [`REFERENCE_WRITE_COMMANDS`]，纯函数 [`is_reference_write`] 承载判定，
//! 命令层统一经 [`emit_reference_changed`] 走该判定；HTTP 端点（账号/分类
//! create/delete）结构上即参考写入，直接经 [`emit_ledger_changed`] 发射。

use tauri::{AppHandle, Emitter};

/// 通用参考数据失效信号事件名（前端订阅；无 payload）。
pub const LEDGER_CHANGED: &str = "ledger:changed";

/// 参考写入 IPC 命令清单：命中即改动参考表，成功后应 emit `ledger:changed`。
/// 新增参考写入命令时同步扩充本清单，并由 [`is_reference_write`] 单测锁定。
const REFERENCE_WRITE_COMMANDS: &[&str] = &[
    "create_account",
    "delete_account",
    "create_category",
    "update_category",
    "reorder_categories",
    "delete_category",
];

/// 薄胶判定：该 IPC 命令是否为参考写入（决定写入成功后是否发失效信号）。
pub fn is_reference_write(command: &str) -> bool {
    REFERENCE_WRITE_COMMANDS.contains(&command)
}

/// 发出 `ledger:changed` 信号（无 payload）。事件发射失败不影响写入结果，静默忽略。
pub fn emit_ledger_changed(app: &AppHandle) {
    let _ = app.emit(LEDGER_CHANGED, ());
}

/// HTTP 端点发射入口：`app` 为 `Option` 仅为集成测试留的缝（不经真实 Tauri 运行时
/// 构建路由，传 `None` 跳过发射）；生产路径由 `start_http_server` 注入 `Some`。
pub fn emit_ledger_changed_if_present(app: &Option<AppHandle>) {
    if let Some(app) = app {
        emit_ledger_changed(app);
    }
}

/// 参考写入 IPC 命令成功后统一入口：以命令名为证据走 [`is_reference_write`] 判定，
/// 命中才发射。判定清单集中在本模块（新增参考写入命令须同步进入
/// [`REFERENCE_WRITE_COMMANDS`]，由单测锁定映射）；交易类命令误接入此处时会被
/// 判定拦下（不 emit）。
pub fn emit_reference_changed(app: &AppHandle, command: &str) {
    if is_reference_write(command) {
        emit_ledger_changed(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_write_commands_are_recognized() {
        for cmd in REFERENCE_WRITE_COMMANDS {
            assert!(is_reference_write(cmd), "「{cmd}」应为参考写入");
        }
    }

    #[test]
    fn non_reference_writes_are_rejected() {
        // 交易类写入与只读命令不得误判为参考写入（交易写入本期不 emit）。
        for cmd in [
            "create_transaction",
            "create_transactions",
            "delete_transaction",
            "list_accounts",
            "list_categories",
            "list_transactions",
            "create_budget",
        ] {
            assert!(!is_reference_write(cmd), "「{cmd}」不应视为参考写入");
        }
    }

    #[test]
    fn event_name_is_generic() {
        assert_eq!(LEDGER_CHANGED, "ledger:changed");
    }
}
