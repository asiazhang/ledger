//! 启动失败接管基座（issue #601 / ADR-0075 决策 5 修订）：库文件启动处置判定
//! 与进程级启动失败门。
//!
//! 启动期数据库打不开（明文库损坏等）不再弹原生「重置/退出」对话框、不再退出：
//! 失败状态经 [`BootFailureGate`] 暴露给前端（IPC 门禁 + HTTP 门禁 + 启动编排
//! 三处共同消费，与 [`super::encryption::EncryptionGate`] 同构），由前端启动
//! 失败恢复屏承担恢复通道（首版通道：重置为空库）。
//!
//! 「等待解锁」与「损坏残留」的区分（本模块判定核心）：头探测的「密文」三态
//! 把任意非明文魔数文件都计为密文（[`super::encryption::DbFileKind::Encrypted`]），
//! 若照单全收，损坏的明文残留会把用户卡在解锁屏。此处沿用启动搬迁的既有实用
//! 判别（`has_encrypted_file_layout` 页对齐形态，`data_location.rs` 先例）：
//! 具备密文库落盘形态的才是真密文库（等待主口令），其余非明文魔数文件按启动
//! 失败处理——SQLCipher 对错误口令与损坏同报 NOTADB、运行期不可靠区分，页对齐
//! 是唯一可用信号（ADR-0075 决策 5 修订注记）。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::Result;

use super::encryption::{self, DbFileKind};

/// 启动失败的稳定错误码（issue #601）：库文件不可读/建连失败共用的单一
/// 码化错误，前端按码本地化失败恢复屏文案，IPC/HTTP 门禁拒绝同码。
pub const BOOT_DB_UNREADABLE: &str = "boot.db-unreadable";

/// 门禁拒绝用的单一错误构造（IPC 壳与 HTTP 壳共用，避免码 + 消息双份漂移）。
pub fn gate_rejection_error() -> crate::error::AppError {
    crate::error::AppError::coded(
        BOOT_DB_UNREADABLE,
        "启动失败，数据库不可用，请先在失败恢复屏重置或恢复",
    )
}

/// 启动失败门（issue #601）：启动期数据库打不开后的进程状态标志。
///
/// 消费方有三处，共同保证「失败恢复屏期间无业务读写」：
/// - IPC 壳门禁（`lib.rs` invoke wrapper）：失败期间仅放行失败恢复屏所需
///   的最小命令面（启动状态查询、重置），其余命令一律拒绝——占位连接不是
///   业务库，任何业务读写都不得触达；
/// - HTTP 壳门禁（`api_server` 中间件）：失败期间数据端点返回码化错误，
///   AI 导入 HTTP 面照常不可用（与锁定期间同口径）；
/// - 启动编排（`lib.rs`）：失败期间不启动自动备份调度（定时追补同轮承载），
///   由重置成功后的恢复编排拉起。
///
/// `Clone` 形态与 [`super::encryption::EncryptionGate`] 同理：同一实例在
/// Builder 装配期被 invoke wrapper 捕获，setup 期登记为应用状态，HTTP 壳
/// 状态持同一实例。
#[derive(Clone)]
pub struct BootFailureGate {
    failed: Arc<AtomicBool>,
}

impl BootFailureGate {
    /// 新建启动失败门（默认未失败；启动失败路径再显式置位）。
    pub fn new() -> Self {
        Self {
            failed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 当前是否处于启动失败（等待前端恢复通道接管）状态。
    pub fn is_failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }

    /// 登记启动失败（启动编排失败路径调用；幂等）。
    pub fn set_failed(&self) {
        self.failed.store(true, Ordering::SeqCst);
    }

    /// 清除启动失败（重置成功、业务可用起点已就位时调用）。
    pub fn clear(&self) {
        self.failed.store(false, Ordering::SeqCst);
    }
}

// clippy::new_without_default 约定形态（无参 new 配套 Default；
// 生产路径只经 `run()` 显式 new，Default 供泛型/工具场景兜底）。
impl Default for BootFailureGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 启动期库文件处置判定（issue #601）：在头探测之上区分三种启动去向。
///
/// - [`BootDisposition::OpenPlaintext`]：明文库/空文件，正常建连（明文日常
///   启动零改动；建连失败由调用方按启动失败处理）；
/// - [`BootDisposition::AwaitUnlock`]：真密文库（头部非明文魔数**且**具备
///   密文库页对齐落盘形态），进入锁定等待解锁（#570 既有路径）；
/// - [`BootDisposition::Unreadable`]：头部非明文魔数且无密文库形态的损坏
///   残留（旧缺陷会把它当密文库卡在解锁屏），按启动失败处理。
///
/// 探测本身的 IO 失败（权限等）原样上抛，由调用方与建连失败同路处理。
pub fn classify_for_boot(path: &Path) -> Result<BootDisposition> {
    match encryption::probe_file_kind(path)? {
        DbFileKind::Plaintext | DbFileKind::Empty => Ok(BootDisposition::OpenPlaintext),
        DbFileKind::Encrypted => {
            if encryption::has_encrypted_file_layout(path) {
                Ok(BootDisposition::AwaitUnlock)
            } else {
                Ok(BootDisposition::Unreadable)
            }
        }
    }
}

/// [`classify_for_boot`] 的三态结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootDisposition {
    /// 明文库/空文件：正常建连路径。
    OpenPlaintext,
    /// 真密文库：进入锁定等待解锁（#570 既有路径）。
    AwaitUnlock,
    /// 损坏残留：按启动失败处理（issue #601）。
    Unreadable,
}

#[cfg(test)]
mod tests;
