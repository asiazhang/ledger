//! 壳层共享用户可见文案常量住所（#2163）。
//!
//! 全壳跨模块复用的用户可见文案词面（任务退出三词面、备份信息 / 加密状态
//! 读取失败前缀）在此单点声明，各视图与引导模块统一 `use` 消费，不再各自
//! 重复声明——#2147 设置面收口、#2149 全壳收口后原住
//! [`crate::settings_data`]，#2163 按模块名揭示角色归位至此（词面零变化，
//! 仅常量搬家 + 三词面补齐 `MSG_` 前缀与同族一致）。
//!
//! 设置域自用的文案常量（恢复弹窗、备份卡等）仍住 [`crate::settings_data`]，
//! 不上收；本模块只收「跨模块共享」的词面。

/// 任务退出三词面单一来源（旧壳命令层同守卫词表）。
pub(crate) const MSG_SESSION_MISSING: &str = "内部错误：账本库会话缺失";
pub(crate) const MSG_READ_TASK_FAILED: &str = "内部错误：读任务异常退出";
pub(crate) const MSG_WRITE_TASK_FAILED: &str = "内部错误：写入任务异常退出";

pub(crate) const MSG_META_FAILED_PREFIX: &str = "读取备份信息失败: ";
pub(crate) const MSG_ENCRYPTION_STATUS_FAILED_PREFIX: &str = "读取加密状态失败: ";
