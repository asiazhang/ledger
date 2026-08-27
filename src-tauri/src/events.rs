//! 事件发射：写入/产物变更后的粗粒度失效信号。
//!
//! - `ledger:changed`（issue #79）：参考数据（`currencies / accounts / categories`）
//!   任一写入成功后由调用方 emit，前端 `useReferenceStore` 订阅后自动重拉三张参考表。
//!   「是否为参考写入」的判定收敛在本模块：IPC 命令清单见 [`REFERENCE_WRITE_COMMANDS`]，
//!   纯函数 [`is_reference_write`] 承载判定，命令层统一经 [`emit_reference_changed`] 走该
//!   判定；HTTP 端点（账号/分类 create/delete）结构上即参考写入，直接经
//!   [`emit_ledger_changed`] 发射。交易类写入不触发。
//! - `ledger:backups-changed`（issue #129）：自动备份完成 / 受管备份清理成功后 emit，
//!   与前者平行、同样无 payload；前端设置页订阅后自动刷新备份列表与自动备份状态。
//!   深路径执行点拿不到 `AppHandle`，经 [`init_event_app`] 注入的镜像句柄发射。

use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

/// 通用参考数据失效信号事件名（前端订阅；无 payload）。
pub const LEDGER_CHANGED: &str = "ledger:changed";

/// 备份产物变更信号事件名（issue #129；无 payload，与 [`LEDGER_CHANGED`] 平行）。
/// 自动备份完成、受管备份清理等改变备份列表的动作成功后由后端发出，
/// 前端设置页据此自动刷新备份列表与自动备份状态。
pub const BACKUPS_CHANGED: &str = "ledger:backups-changed";

/// 进程级应用句柄镜像：自动备份的深路径执行点（如写时顺带检查
/// [`crate::auto_backup::on_write`]）只持有 `&Connection`，拿不到 Tauri 的
/// `AppHandle`——启动时经 [`init_event_app`] 注入一次，深层执行点经
/// [`emit_backups_changed_current`] 发射；未注入（单测环境）时静默跳过。
static EVENT_APP: OnceLock<AppHandle> = OnceLock::new();

/// 启动时注入应用句柄（仅 setup 调用一次；重复调用以首次为准）。
pub fn init_event_app(app: &AppHandle) {
    let _ = EVENT_APP.set(app.clone());
}

/// 发出 `ledger:backups-changed` 信号（无 payload）。事件发射失败不影响写入结果，静默忽略。
pub fn emit_backups_changed(app: &AppHandle) {
    let _ = app.emit(BACKUPS_CHANGED, ());
}

/// 深路径发射入口：经进程级镜像句柄发出 [`BACKUPS_CHANGED`] 信号，
/// 句柄未注入时静默跳过。供拿不到 `AppHandle` 的深层执行点使用；
/// 命令层持有真实句柄时应直接走 [`emit_backups_changed`]。
pub fn emit_backups_changed_current() {
    if let Some(app) = EVENT_APP.get() {
        emit_backups_changed(app);
    }
}

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

    #[test]
    fn backups_changed_event_name() {
        // 与 ledger:changed 平行：同一命名空间、无 payload 的粗粒度信号（issue #129）。
        assert_eq!(BACKUPS_CHANGED, "ledger:backups-changed");
        assert!(BACKUPS_CHANGED.starts_with("ledger:"));
    }

    /// 未注入句柄时深路径发射静默跳过，不 panic。
    #[test]
    fn emit_backups_changed_current_without_handle_is_silent() {
        // OnceLock 进程内单次注入；在未初始化的测试环境无句柄可用即可重复调用。
        if EVENT_APP.get().is_none() {
            emit_backups_changed_current();
        }
    }
}
