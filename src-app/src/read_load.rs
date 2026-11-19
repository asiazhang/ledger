//! 读装载守卫（#2154）：`db 缺席守卫 + blocking + Err 映射` 三段形状的单点
//! 收口。
//!
//! 投资族读装载与物品清单装载的既有三段形状——
//! `let Some(db) = db else { error.set(…) }` + `blocking(…)` + `Ok(Err(e))` /
//! `Err(_)` 两臂错误映射——逐份复制六处（[`crate::investments`]×3、
//! [`crate::investments_holdings`]、[`crate::investments_ledger`]、
//! [`crate::items`] reload）。本模块把「缺席守卫 + 阻塞读 + 三分口产出」收为
//! [`blocking_read`]；文案映射留在调用点——ledger 的加载失败前缀与 items 的
//! 折叠语义是各自的真实差异，不进收口。码化错误语义零变化：域错误原样经
//! [`ReadLoad::Coded`] 上抛，`Display` 面与在场行为（信号置序、loading 收口）
//! 不变。
//!
//! 不收口的变体（留痕）：[`crate::investments`] 的 `load_reference`（静默
//! 弃误变体，三段形状缺第三段错误映射——收口需引入无谓失败分口）；
//! [`crate::items`] 的 `perform_recalc`（db 缺席路径在阻塞闭包内构造码化
//! 错误，非守卫形状）；其余视图模块（transactions / search / dashboard 等）
//! 的同名形状不在本票范围。写路径的同族孪生形状（db 槽位 `None` → 码化
//! SESSION_MISSING 错误）另收口于 [`crate::write_load`]（#2162）。

use std::sync::Arc;

use ledger_infra::db::DbState;
use ledger_infra::error::AppError;
use nsh_core::element::blocking;

/// 读装载守卫产出：成功落数据，三失败分口一一对应旧三段形状（缺席守卫
/// `else` 臂 / `Ok(Err(e))` 臂 / `Err(_)` 臂）。
pub(crate) enum ReadLoad<T> {
    /// 读命令成功：快照原样交调用方落信号。
    Ready(T),
    /// 码化错误（旧 `Ok(Err(e))` 臂）：域错误原样携带，文案归调用方。
    Coded(AppError),
    /// 会话句柄缺席（旧 `let Some(db) = db else` 臂）：db 槽位 `None`。
    MissingDb,
    /// 阻塞读任务异常退出（旧 `Err(_)` 臂）：JoinError。
    TaskFailed,
}

/// 读装载守卫（#2154 三段形状收口）：db 槽位缺席直接产出
/// [`ReadLoad::MissingDb`]；在位则整段读命令进 [`blocking`] 后台线程
/// （SQLite IO 不占引擎线程，旧形状同口径），三分口产出。
pub(crate) async fn blocking_read<T, F>(db: Option<Arc<DbState>>, read: F) -> ReadLoad<T>
where
    T: Send + 'static,
    F: FnOnce(&DbState) -> ledger_infra::error::Result<T> + Send + 'static,
{
    let Some(db) = db else {
        return ReadLoad::MissingDb;
    };
    match blocking(move || read(&db)).await {
        Ok(Ok(value)) => ReadLoad::Ready(value),
        Ok(Err(e)) => ReadLoad::Coded(e),
        Err(_) => ReadLoad::TaskFailed,
    }
}
