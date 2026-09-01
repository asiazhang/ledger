//! 事件发射：写入/产物变更后的粗粒度失效信号。
//!
//! 本模块只承载**机制**（事件名常量 + `emit_*` 发射入口 + `EVENT_APP` 镜像句柄）；
//! 「哪个写操作发哪个信号」的**知识**已收拢到信号映射单点 `signals::signals_for`
//! （ADR-0044）：壳层经 `signals::emit_for` 判定后走到这里发射。
//!
//! - `ledger:changed`（issue #79）：参考数据（`currencies / accounts / categories / merchants`）
//!   任一写入成功后由调用方 emit，前端 `useReferenceStore` 订阅后自动重拉参考表。
//!   发射判定在映射单点：两壳（IPC / HTTP）全部写命令经 `signals::emit_for`
//!   按写操作身份 + 结果证据判定后发射（ADR-0044 / #332-#334）；交易类写入基线零信号
//!   （唯一例外——交易写「即建商户」——同样经 `signals` 映射携证据发射，
//!   ADR-0044 / issue #331）。旧字符串白名单机制（`REFERENCE_WRITE_COMMANDS` /
//!   `is_reference_write` / `emit_reference_changed` / `emit_ledger_changed_if_present`）
//!   已随 #335 收缩删除，「谁发什么」的判定知识唯一载体是 `signals` 映射单点，
//!   壳侧接线由两壳声明表 × 映射表交叉核对兜底（`signals_cross_check`）。
//! - `ledger:backups-changed`（issue #129）：自动备份完成 / 受管备份清理成功后 emit，
//!   与前者平行、同样无 payload；前端设置页订阅后自动刷新备份列表与自动备份状态。
//!   深路径执行点拿不到 `AppHandle`，经 [`init_event_app`] 注入的镜像句柄发射。
//! - `ledger:prices-changed`（ADR-0031，issue #236）：行情同步命令写入价格后 emit——
//!   增量 `sync_holding_prices` 成功且实际写入、全量 `sync_instruments` 结束且有落库
//!   （含用户中断，中断保留已落库价格）；与前者平行、同样无 payload，前端价格消费方
//!   各自订阅后重拉自身数据。「是否 emit」的判定单点在 `signals` 映射
//!   （ADR-0044，#333 起价格域四命令壳层经 `emit_for` 归一化证据后发射），
//!   本模块只承载事件名与发射入口。

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
/// 任一落点时（issue #291 / ADR-0036，生产者清单再添一处；证据归一化与「是否发」
/// 判定单点见 `signals` 映射，ADR-0044 / issue #333）；前端价格消费方
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
/// 「是否 emit」不在本模块判定：壳层经 `signals` 映射单点判定（ADR-0044 / issue #333）
/// 后才走到这里。
pub fn emit_prices_changed(app: &AppHandle) {
    let _ = app.emit(PRICES_CHANGED, ());
}

/// 发出 `ledger:changed` 信号（无 payload）。事件发射失败不影响写入结果，静默忽略。
/// 「发不发」不在本模块判定：壳层经 `signals` 映射单点判定（ADR-0044）后才走到这里。
pub fn emit_ledger_changed(app: &AppHandle) {
    let _ = app.emit(LEDGER_CHANGED, ());
}

#[cfg(test)]
mod tests {
    use super::*;

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
