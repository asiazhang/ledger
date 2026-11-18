//! 写路径会话缺失守卫（#2162）：`db 槽位 None → 码化 SESSION_MISSING 错误`
//! 孪生形状的单点收口，读路径守卫 [`crate::read_load`]（#2154）的同族。
//!
//! 视图写路径的既有孪生形状——`blocking(move || match db.as_ref() { …
//! None => Err(AppError::coded(boot::CODE_SESSION_MISSING, MSG_SESSION_MISSING)) })`
//! 的 `None` 分臂（含 tuple match 的 `(None, _)` 分臂与
//! `let Some(db) = db else` 早退变体，None 语义同为 db 槽位缺席）——逐处
//! 复制 72 处、19 个视图文件（accounts / items / merchants / policies /
//! savings / transactions / settings_* / scheduled_* / investments_form /
//! txn_form / physical_assets / budget / insurers / backup_entry）。本模块
//! 把会话缺失的错误构造收为 [`session_missing_err`]；错误码与文案的单一
//! 来源仍在 [`crate::boot`] 码表与 [`crate::shell_msgs`]——收口只收构造
//! 点，不改码化语义与词面。
//!
//! 不收口的变体（留痕）：各卡模块的
//! `CODE_FILE_DIALOG_FAILED`（文件对话框打开失败，非会话缺失）；账本注册
//! 表三动作的 `"book.registry-unavailable"` / `BOOKS_SESSION_UNAVAILABLE`
//! （注册表不可用，码与文案均不同）；[`crate::boot`] 自身的
//! `locked_session_db_path` 与数据安全面 `db_path_for_data_safety`（引导
//! 记录 / 启动参数目录缺失的兜底，None 语义源不同且位于本模块下游，反向
//! 依赖不成立）。
//!
//! #2162 review 留痕（收口范围与 read_load 的边界）：本模块收口对象是
//! 「db 槽位 None 分臂的码化错误构造」这一形状——无论外层 blocking 闭包
//! 承载读任务还是写任务，形状同构即归此，故备份卡（#2167 拆分后住
//! [`crate::backup_cards`]，原 [`crate::settings_data`]）的读任务位点
//!（refresh_rows_task / refresh_auto_backup_state /
//! pick_restore_task 读 meta 等）消费 [`session_missing_err`] 也算收口；
//! [`crate::read_load`]（#2154）收的是视图装载「缺席守卫 + blocking +
//! 三分口产出」三段形状，口径更窄，两者的收口范围不同但不冲突。
//! settings_data（拆分后各卡模块同位）读任务位点只共享 None 分臂构造、
//! 不构成三段形状，故归本模块而非 read_load。

use ledger_infra::error::AppError;

use crate::boot;
use crate::shell_msgs::MSG_SESSION_MISSING;

/// 写路径会话缺失守卫（#2162 收口）：db 槽位 `None` 分臂的码化错误构造单点
/// ——`AppError::coded(boot::CODE_SESSION_MISSING, MSG_SESSION_MISSING)`，
/// 码与文案逐字同旧孪生形状。
pub(crate) fn session_missing_err() -> AppError {
    AppError::coded(boot::CODE_SESSION_MISSING, MSG_SESSION_MISSING)
}
