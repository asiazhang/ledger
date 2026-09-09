//! Transport 同步通道（issue #859 / ADR-0091 决策 1）：哑字节通道抽象。
//!
//! 通道只搬运同步文件、不理解载荷、不做任何合并与冲突处理——智能全在各端
//! （ADR-0091 决策 1/2）。两端以同一抽象对接，桌面与移动端共享同一代码路径
//! （域不依赖壳，ADR-0056）；v1 内置 WebDAV 后端（[`webdav`]），其余后端是
//! 可逆工程决策。
//!
//! 路径语义：`path` 是相对同步根的逻辑路径（`/` 分隔，如
//! `book-<id>/streams/<device>/seg-...enc`），由通道实现映射到实际地址
//! （WebDAV URL / 对象键 / 文件路径）。文件名集合由 [`super::channel`] 的
//! 布局规则产出，全部为 URL 安全字符。
//!
//! 错误语义：凭据错误与网络失败均产生**明确可重试**的码化错误
//! （`sync-channel.auth-failed` / `sync-channel.network-failed`），修正凭据
//! 或网络恢复后重试同步轮次即可；失败不影响本地记账（同步是旁路写入）。

pub mod webdav;

use crate::error::AppError;

/// 哑字节通道：同步文件搬运的最小操作面。
///
/// 写入恒为整体覆盖（先封包后整体 PUT，不做追加以规避网盘弱原子性）；
/// 目录逐级确保存在（WebDAV 无递归建目录）。实现须可跨线程共享
/// （同步轮次在后台线程执行，#862 壳层编排）。
pub trait Transport: Send + Sync {
    /// 逐级确保目录存在（幂等：已存在不算错误）。
    fn ensure_dir(&self, path: &str) -> crate::error::Result<()>;

    /// 读取文件；不存在返回 `None`（manifest 驱动的增量拉取以 404 为常态输入）。
    fn read_file(&self, path: &str) -> crate::error::Result<Option<Vec<u8>>>;

    /// 整体写入文件（覆盖语义；调用方保证内容完整后写入）。
    fn write_file(&self, path: &str, bytes: &[u8]) -> crate::error::Result<()>;
}

/// 凭据被拒（HTTP 401/403）的码化错误单点：明确、可重试（修正凭据后）。
pub(super) fn auth_failed_error() -> AppError {
    AppError::coded(
        "sync-channel.auth-failed",
        "同步通道凭据被拒绝，请检查网盘账号与密码（或应用密码）设置",
    )
}

/// 网络失败（连接/超时/DNS）的码化错误单点：明确、可重试（网络恢复后）。
pub(super) fn network_failed_error(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.network-failed",
        format!("同步通道网络失败，请检查网络后重试: {detail}"),
        &[detail],
    )
}

/// 非预期 HTTP 状态的码化错误单点（网盘服务异常等，可重试）。
pub(super) fn http_failed_error(status: u16, detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.http-failed",
        format!("同步通道服务异常（HTTP {status}）: {detail}"),
        &[&status.to_string(), detail],
    )
}
