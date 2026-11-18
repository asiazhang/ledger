//! 壳层共享用户可见文案常量住所（#2163）。
//!
//! 全壳跨模块复用的用户可见文案词面（任务退出三词面、备份信息 / 加密状态
//! 读取失败前缀）在此单点声明，各视图与引导模块统一 `use` 消费，不再各自
//! 重复声明——#2147 设置面收口、#2149 全壳收口后原住
//! [`crate::settings_data`]，#2163 按模块名揭示角色归位至此（词面零变化，
//! 仅常量搬家 + 三词面补齐 `MSG_` 前缀与同族一致）。
//!
//! #2165：不同句的码化错误句面（读页 / 删除 / 创建 / 保存 / 创建账本 /
//! 更新检查 / 下载 / 启动 / 解锁 / 重置 / 备份任务异常退出、改价目标缺失）
//! 同族扩容至此，内联字面量与本地常量一并收口（词面零变化）；外层
//! 「删除失败: 」等前缀仍留调用点。
//!
//! 设置域自用的文案常量（恢复弹窗、备份卡等）不上收——#2167 拆分后随
//! 各卡归位（[`crate::backup_cards`] / [`crate::encryption_card`] 等）；本
//! 模块只收「跨模块共享」的词面。恢复失败前缀
//! [`MSG_RESTORE_FAIL_PREFIX`] 词面源自恢复弹窗，但被 [`crate::unlock`] 跨
//! 模块消费，同属跨模块共享词面归此（#2162–#2165 review 收口）。

/// 任务退出三词面单一来源（旧壳命令层同守卫词表）。
pub(crate) const MSG_SESSION_MISSING: &str = "内部错误：账本库会话缺失";
pub(crate) const MSG_READ_TASK_FAILED: &str = "内部错误：读任务异常退出";
pub(crate) const MSG_WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

/// 任务异常退出句面族（「内部错误：N 任务异常退出」，N = 业务动作；
/// 旧壳各命令 Err(_) 臂同词面，#2165 收口）。
pub(crate) const MSG_READ_PAGE_TASK_FAILED: &str = "内部错误：读页任务异常退出";
pub(crate) const MSG_CREATE_TASK_FAILED: &str = "内部错误：创建任务异常退出";
pub(crate) const MSG_SAVE_TASK_FAILED: &str = "内部错误：保存任务异常退出";
pub(crate) const MSG_DELETE_TASK_FAILED: &str = "内部错误：删除任务异常退出";
pub(crate) const MSG_CREATE_BOOK_TASK_FAILED: &str = "内部错误：创建账本任务异常退出";
pub(crate) const MSG_STARTUP_TASK_FAILED: &str = "内部错误：启动任务异常退出";
pub(crate) const MSG_UNLOCK_TASK_FAILED: &str = "内部错误：解锁任务异常退出";
pub(crate) const MSG_RESET_TASK_FAILED: &str = "内部错误：重置任务异常退出";
pub(crate) const MSG_BACKUP_TASK_FAILED: &str = "内部错误：备份任务异常退出";
pub(crate) const MSG_UPDATE_CHECK_TASK_FAILED: &str = "内部错误：更新检查任务异常退出";
pub(crate) const MSG_DOWNLOAD_TASK_FAILED: &str = "内部错误：下载任务异常退出";

/// 改价目标缺失句面（批内改价第一跳目标在场的不可达兑底码化拦场，
/// #2165 收口；与 [`crate::policies`] 的 CODE_REBUILD_TARGET_MISSING 配对）。
pub(crate) const MSG_REPRICE_TARGET_MISSING: &str = "内部错误：改价目标缺失";

pub(crate) const MSG_META_FAILED_PREFIX: &str = "读取备份信息失败: ";
pub(crate) const MSG_ENCRYPTION_STATUS_FAILED_PREFIX: &str = "读取加密状态失败: ";

/// 恢复失败前缀（恢复弹窗错误行；词面源自恢复弹窗但被 [`crate::unlock`]
/// 跨模块消费，#2162–#2165 review 自 [`crate::settings_data`] 归位至此）。
pub(crate) const MSG_RESTORE_FAIL_PREFIX: &str = "恢复失败: ";
