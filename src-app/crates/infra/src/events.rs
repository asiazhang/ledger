//! 事件发射：写入/产物变更后的粗粒度失效信号 + 非信号的带 payload 事件发射。
//!
//! 失效信号的事件名常量、`emit_*` 发射入口、深路径发射槽、发射器接缝
//! [`SignalEmitter`] 机制收口于本模块；「哪个写操作发哪个信号」的**知识**已收拢到
//! 信号映射单点 `signals::signals_for`（ADR-0044）：壳层经 `signals::emit_for`
//! 判定后走到这里发射。发射一律经 [`SignalEmitter::post`] **非阻塞交接**
//!（tauri 类型件——AppHandle 生产实现、`post_emit_with` 主线程投递与
//! `init_event_app` 注入入口——随 Tauri 壳退役删除，#2083；失效信号的唯一生产
//! 消费面归 NSH 壳的信号广播，深路径发射槽经 [`init_deep_path_emitter`] 注册点
//! 注入）（机理与死锁背景见 ADR-0054，历史：IPC 壳、HTTP 壳与深路径镜像句柄
//! 三条失效信号发射路径曾共用主线程投递机制，一处改动全壳生效）。
//! 不经失效信号映射的带 payload 事件（标的同步进度
//! `ledger:instrument-sync-progress`，issue #897）事件名常量与发射入口归
//! 领域侧（`sync` 域的 `progress` 模块），本模块不承载其知识。发射器接缝
//!（[`SignalEmitter`]，spec #366）是「非阻塞」约定的类型化载体与回归测试注入点。
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
//!   深路径执行点拿不到 `AppHandle`，经启动注入的深路径发射器（见
//!   [`init_deep_path_emitter`]；旧壳经门内 `init_event_app` 装入）发射。
//! - `ledger:prices-changed`（ADR-0031，issue #236）：行情同步命令写入价格后 emit——
//!   标的信息同步 `sync_instrument_info` 成功且实际写入（全量同步已随 ADR-0081 决策 3 退役，
//!   issue #698）；与前者平行、同样无 payload，前端价格消费方
//!   各自订阅后重拉自身数据。「是否 emit」的判定单点在 `signals` 映射
//!   （ADR-0044，#333 起价格域四命令壳层经 `emit_for` 归一化证据后发射），
//!   本模块只承载事件名与发射入口。

use std::sync::OnceLock;

/// 通用参考数据失效信号事件名（前端订阅；无 payload）。
pub const LEDGER_CHANGED: &str = "ledger:changed";

/// 备份产物变更信号事件名（issue #129；无 payload，与 [`LEDGER_CHANGED`] 平行）。
/// 自动备份完成、受管备份清理等改变备份列表的动作成功后由后端发出，
/// 前端设置页据此自动刷新备份列表与自动备份状态。
pub const BACKUPS_CHANGED: &str = "ledger:backups-changed";

/// 价格数据变更信号事件名（ADR-0031，issue #236；无 payload，与 [`LEDGER_CHANGED`] /
/// [`BACKUPS_CHANGED`] 平行，同一 `ledger:*` 命名空间、同一 `<domain 复数>-changed` 风格）。
/// 语义锚「价格数据已变更」，覆盖 MarketPrice / PriceHistory / FxRateHistory；生产者：
/// 行情同步命令按判定发出（增量实际写入；全量同步已退役，issue #698）、场外基金按代码即拉
/// 落现价缓存时（issue #301 / ADR-0038，未取到净值不广播）与手动报价实际写入
/// 任一落点时（issue #291 / ADR-0036，生产者清单再添一处；证据归一化与「是否发」
/// 判定单点见 `signals` 映射，ADR-0044 / issue #333）；前端价格消费方
/// 各自订阅后重拉自身数据。
pub const PRICES_CHANGED: &str = "ledger:prices-changed";

/// 进程级深路径发射器（ADR-0112 决策 5 注册点形态）：自动备份的
/// 深路径执行点（连接层写入口 [`crate::db::write_locked`] 提交点的写时顺带检查，
/// ADR-0032）只持有 `&Connection`，拿不到壳层发射器——启动时经
/// [`init_deep_path_emitter`] 注入一次（实现由壳层提供），
/// 深层执行点经 [`emit_backups_changed_current`] 发射；未注入（单测环境 / 无
/// 消费面的壳）时静默跳过。
static DEEP_EMITTER: OnceLock<Box<dyn SignalEmitter>> = OnceLock::new();

/// 注入深路径发射器（仅壳层启动调用一次；重复调用以首次为准）。
pub fn init_deep_path_emitter(emitter: Box<dyn SignalEmitter>) {
    let _ = DEEP_EMITTER.set(emitter);
}

/// 发射器接缝（spec #366）：「把一个失效信号事件投递出去」的机制抽象。
/// 唯一实现约定：**非阻塞**——`post` 只把发射动作交出去（投递 / 入队）即返回，
/// 绝不等事件真正送达；投递或发射失败静默忽略，不影响写事务结果（ADR-0044
/// 「发射失败静默」语义）。写路径「不被发射阻塞」的外部保证建立在本约定上：
/// 回归测试（`signals::tests::emit_blocking`，spec #366）注入可阻塞假发射器
/// 钉死之——发射器阻塞期间写路径仍及时返回，放行后信号最终到达、不丢失。
pub trait SignalEmitter: Send + Sync {
    /// 投递一个事件。实现必须非阻塞：交接即返回，不等发射完成。
    fn post(&self, event: &'static str);
}

/// 深路径发射入口：经进程级镜像句柄发出 [`BACKUPS_CHANGED`] 信号，
/// 句柄未注入时静默跳过。供拿不到壳层发射器的深层执行点使用；
/// 与 [`SignalEmitter`] 生产实现共用同一非阻塞约定——深路径执行点（任意写线程）
/// 不等回执、不阻塞写事务（spec #364）。
pub fn emit_backups_changed_current() {
    if let Some(emitter) = DEEP_EMITTER.get() {
        emitter.post(BACKUPS_CHANGED);
    }
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

    /// 未注入发射器时深路径发射静默跳过，不 panic。
    #[test]
    fn emit_backups_changed_current_without_emitter_is_silent() {
        // OnceLock 进程内单次注入；在未初始化的测试环境无发射器可用即可重复调用。
        if DEEP_EMITTER.get().is_none() {
            emit_backups_changed_current();
        }
    }
}
