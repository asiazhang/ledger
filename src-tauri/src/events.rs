//! 事件发射：写入/产物变更后的粗粒度失效信号 + 非信号的带 payload 事件发射。
//!
//! 失效信号的事件名常量、`emit_*` 发射入口、`EVENT_APP` 镜像句柄、发射器接缝
//! [`SignalEmitter`] 与主线程非阻塞投递 [`post_emit_with`] 机制收口于本模块；
//! 「哪个写操作发哪个信号」的**知识**已收拢到信号映射单点 `signals::signals_for`
//!（ADR-0044）：壳层经 `signals::emit_for` 判定后走到这里发射。发射一律经
//! [`SignalEmitter::post`] / [`post_emit_with`] **投递到主线程事件循环队尾**
//! 非阻塞执行（机理与死锁背景见其文档，spec #364 / ADR-0054）：IPC 壳、HTTP 壳
//! 与深路径镜像句柄三条失效信号发射路径共用这一处投递机制，一处改动全壳生效。
//! 不经失效信号映射的带 payload 事件（行情同步进度 `sync-instruments:progress`，
//! issue #369）只共用 [`post_emit_with`] 投递机制——其事件名常量与发射入口归
//! 领域侧（`commands::sync` 的 `emit_progress`），本模块不承载其知识，
//! 投递机制不另起第二套。发射器接缝（[`SignalEmitter`]，spec #366）是
//! 「非阻塞」约定的类型化载体与回归测试注入点。
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

/// 机制收口单点（spec #364 / ADR-0054）：把「发射动作」闭包投递到
/// **主线程事件循环队尾**执行——`AppHandle::run_on_main_thread` 非阻塞入队
/// （tauri `send_user_message` 只投递不等回执），调用即返回、不等发射完成。
/// 泛化为收任意发射动作闭包（issue #369）：失效信号经 [`SignalEmitter`] 接缝
/// （[`AppHandle`] 实现构造 emit 闭包）走同一机制；带 payload 的事件（如行情
/// 同步进度，不经失效信号映射 ADR-0044）由领域侧在此构造 `emit` 闭包，
/// 不另起第二套投递机制。
///
/// 为什么必须投递而不就地发射：tauri 的 `app.emit`（`tracing` feature 下走
/// `eval_script` 的 `rx.recv()` 回执路径）内部会**同步等待主线程**执行 JS 注入，
/// 并在等待期间持有 `webviews_lock`；写线程（IPC 命令线程 / HTTP tokio worker /
/// 深路径执行点 / 行情同步后台线程）就地发射时，若主线程恰在处理 WebKit URL
/// scheme 回调并抢同一把锁，即成「主线程等锁、写线程等回执」的跨线程死锁
///（spec #364 事故）。投递后 emit 在主线程自己的事件循环里执行（自身线程上
/// `run_on_main_thread` 语义为内联顺序执行，单线程无锁竞争、无自等待），死锁
/// 闭环从机制上消除；调用线程只承担一次非阻塞入队的成本，及时返回。
///
/// 投递失败（应用退出中事件循环已关）与发射本身失败一样静默忽略，不影响写
/// 事务 / 同步结果（ADR-0044「发射失败静默」语义）。同一线程先后入队的动作
/// 按入队顺序在主线程依次执行，事件间无乱序。
pub(crate) fn post_emit_with(app: &AppHandle, action: impl FnOnce() + Send + 'static) {
    let _ = app.run_on_main_thread(action);
}

/// 发射器接缝（spec #366）：「把一个失效信号事件投递出去」的机制抽象。
/// 唯一实现约定：**非阻塞**——`post` 只把发射动作交出去（投递 / 入队）即返回，
/// 绝不等事件真正送达；投递或发射失败静默忽略，不影响写事务结果（ADR-0044
/// 「发射失败静默」语义）。写路径「不被发射阻塞」的外部保证建立在本约定上：
/// 回归测试（`signals::emit_blocking_tests`，spec #366）注入可阻塞假发射器
/// 钉死之——发射器阻塞期间写路径仍及时返回，放行后信号最终到达、不丢失。
pub trait SignalEmitter: Send + Sync {
    /// 投递一个事件。实现必须非阻塞：交接即返回，不等发射完成。
    fn post(&self, event: &'static str);
}

/// 生产实现（spec #364 / ADR-0054）：主线程非阻塞投递——`post` 构造「emit 指定
/// 事件」闭包交 [`post_emit_with`] 投递主线程队尾，调用即返回、不等发射完成。
/// 失效信号的壳层发射入口（`signals::emit_for` / 深路径 `emit_backups_changed`）
/// 全部汇聚于此。
impl SignalEmitter for AppHandle {
    fn post(&self, event: &'static str) {
        let handle = self.clone();
        post_emit_with(self, move || {
            let _ = handle.emit(event, ());
        });
    }
}

/// 发出 `ledger:backups-changed` 信号（无 payload）。经 [`SignalEmitter::post`]
/// 投递主线程非阻塞执行（spec #364）；发射失败不影响写入结果，静默忽略。
pub fn emit_backups_changed(app: &AppHandle) {
    app.post(BACKUPS_CHANGED);
}

/// 深路径发射入口：经进程级镜像句柄发出 [`BACKUPS_CHANGED`] 信号，
/// 句柄未注入时静默跳过。供拿不到 `AppHandle` 的深层执行点使用；
/// 命令层持有真实句柄时应直接走 [`emit_backups_changed`]。
/// 与 [`SignalEmitter`] 生产实现共用同一投递机制——深路径执行点（任意写线程）
/// 同样不等回执、不持 `webviews_lock`（spec #364）。
pub fn emit_backups_changed_current() {
    if let Some(app) = EVENT_APP.get() {
        emit_backups_changed(app);
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

    /// 未注入句柄时深路径发射静默跳过，不 panic。
    #[test]
    fn emit_backups_changed_current_without_handle_is_silent() {
        // OnceLock 进程内单次注入；在未初始化的测试环境无句柄可用即可重复调用。
        if EVENT_APP.get().is_none() {
            emit_backups_changed_current();
        }
    }
}
