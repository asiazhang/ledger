//! 事件发射：写入/产物变更后的粗粒度失效信号。
//!
//! 本模块只承载**机制**（事件名常量 + `emit_*` 发射入口 + `EVENT_APP` 镜像句柄）；
//! 「哪个写操作发哪个信号」的**知识**已收拢到信号映射单点 `signals::signals_for`
//! （ADR-0044）：壳层经 `signals::emit_for` 判定后走到这里发射。
//!
//! - `ledger:changed`（issue #79）：参考数据（`currencies / accounts / categories / merchants`）
//!   任一写入成功后由调用方 emit，前端 `useReferenceStore` 订阅后自动重拉参考表。
//!   发射判定已迁往映射单点：IPC 参考写命令、物品写命令（独立域复用同名事件，
//!   ADR-0014）与余额调整（黑洞即建证据）均经 `signals::emit_for` 判定后发射；
//!   HTTP 端点（账号/分类 create/delete）直接经 [`emit_ledger_changed`] 发射
//!   （判定归拢待 #334）。交易写「即建商户」经映射单点携证据发射（ADR-0044 / #331）。
//!   旧字符串白名单（[`REFERENCE_WRITE_COMMANDS`] / [`is_reference_write`] /
//!   [`emit_reference_changed`]）IPC 生产调用点已清零，按 #330 约定暂留，
//!   待 #335 统一收缩删除。
//! - `ledger:backups-changed`（issue #129）：自动备份完成 / 受管备份清理成功后 emit，
//!   与前者平行、同样无 payload；前端设置页订阅后自动刷新备份列表与自动备份状态。
//!   深路径执行点拿不到 `AppHandle`，经 [`init_event_app`] 注入的镜像句柄发射。
//! - `ledger:prices-changed`（ADR-0031，issue #236）：行情同步命令写入价格后 emit——
//!   增量 `sync_holding_prices` 成功且实际写入、全量 `sync_instruments` 结束且有落库
//!   （含用户中断，中断保留已落库价格）；与前者平行、同样无 payload，前端价格消费方
//!   各自订阅后重拉自身数据。「是否 emit」的判定收敛在同步命令侧
//!   （`commands::sync::should_emit_prices_changed`），本模块只承载事件名与发射入口
//!   （判定归拢映射单点待 #333）。

use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

/// 通用参考数据失效信号事件名（前端订阅；无 payload）。
pub const LEDGER_CHANGED: &str = "ledger:changed";

/// 备份产物变更信号事件名（issue #129；无 payload，与 [`LEDGER_CHANGED`] 平行）。
/// 自动备份完成、受管备份清理等改变备份列表的动作成功后由后端发出，
/// 前端设置页据此自动刷新备份列表与自动备份状态。
pub const BACKUPS_CHANGED: &str = "ledger:backups-changed";

/// 价格数据变更信号事件名（ADR-0031，issue #236；无 payload，与 [`LEDGER_CHANGED`] /
/// [`BACKUPS_CHANGED`] 平行，同一 `ledger:*` 命名空间、同一 `<domain 复数>-changed` 风格）。
/// 语义锚「价格数据已变更」，覆盖 MarketPrice / PriceHistory / FxRateHistory；生产者：
/// 两个行情同步命令按判定发出（增量实际写入 / 全量有落库）、场外基金按代码即拉
/// 落现价缓存时（issue #301 / ADR-0038，未取到净值不广播）与手动报价实际写入
/// 任一落点时（issue #291 / ADR-0036，生产者清单再添一处；判定见
/// `commands::investment::manual_price::should_emit_prices_changed`）；前端价格消费方
/// 各自订阅后重拉自身数据。
pub const PRICES_CHANGED: &str = "ledger:prices-changed";

/// 进程级应用句柄镜像：自动备份的深路径执行点（连接层写入口
/// [`crate::db::write`] 提交点的写时顺带检查，ADR-0032）只持有 `&Connection`，
/// 拿不到 Tauri 的 `AppHandle`——启动时经 [`init_event_app`] 注入一次，深层执行点经
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

/// 发出 `ledger:prices-changed` 信号（无 payload）。事件发射失败不影响同步结果，静默忽略。
/// 「是否 emit」不在本模块判定：两同步命令经 `commands::sync::should_emit_prices_changed`
/// 统一判定后才走到这里。
pub fn emit_prices_changed(app: &AppHandle) {
    let _ = app.emit(PRICES_CHANGED, ());
}

/// 参考写入 IPC 命令清单：命中即改动参考表，成功后应 emit `ledger:changed`。
/// 旧机制遗留（生产调用点已清零，见下），新增写命令不再扩充本清单——
/// 改在 `signals::WriteOp` 声明身份并入映射（ADR-0044）；本清单仅由既有单测锁定。
const REFERENCE_WRITE_COMMANDS: &[&str] = &[
    "create_account",
    "update_account",
    "delete_account",
    "create_category",
    "update_category",
    "reorder_categories",
    "delete_category",
    "create_merchant",
    "update_merchant",
    "delete_merchant",
];

/// 薄胶判定：该 IPC 命令是否为参考写入（决定写入成功后是否发失效信号）。
///
/// **旧机制，生产调用点已清零**（#332 起 IPC 壳改走 `signals::signals_for` 映射
/// 单点，ADR-0044）；按 #330 约定暂留，待 #335 统一收缩删除，勿在新代码中调用。
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
/// 命中才发射。
///
/// **旧机制，生产调用点已清零**（#332 起 IPC 壳改走 `signals::emit_for` 映射单点，
/// ADR-0044）；按 #330 约定暂留，待 #335 统一收缩删除，勿在新代码中调用。
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
        // 交易类写入与只读命令不得误判为参考写入（旧清单不含交易写；「即建商户」
        // 例外走 signals 映射单点，不经本判定，ADR-0044 / issue #331）。
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

    #[test]
    fn prices_changed_event_name() {
        // 与 ledger:changed / ledger:backups-changed 平行：同一命名空间、无 payload
        // 的粗粒度信号（ADR-0031，issue #236），命名随 `<domain 复数>-changed` 风格。
        assert_eq!(PRICES_CHANGED, "ledger:prices-changed");
        assert!(PRICES_CHANGED.starts_with("ledger:"));
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
