//! Transport 同步通道（issue #859 / ADR-0091 决策 1）：哑字节通道抽象。
//!
//! 通道只搬运同步文件、不理解载荷、不做任何合并与冲突处理——智能全在各端
//! （ADR-0091 决策 1/2）。两端以同一抽象对接，桌面与移动端共享同一代码路径
//! （域不依赖壳，ADR-0056）；内置 WebDAV（[`webdav`]）与 S3 兼容对象存储
//! （[`s3`]）后端，其余后端是可逆工程决策。
//!
//! 路径语义：`path` 是相对同步根的逻辑路径（`/` 分隔，如
//! `book-<id>/streams/<device>/seg-...enc`），由通道实现映射到实际地址
//! （WebDAV URL / 对象键 / 文件路径）。文件名集合由 [`super::channel`] 的
//! 布局规则产出，全部为 URL 安全字符。
//!
//! 错误语义（issue #1219 五层分层）：**凭据错误**（`sync-channel.auth-failed`）、
//! **目标不存在**（`sync-channel.target-missing`）、**权限不足**
//! （`sync-channel.permission-denied`）、**网络不可达**
//! （`sync-channel.network-failed`）与服务异常（`sync-channel.http-failed`）各成
//! 一层，层与层给用户的下一步动作不同（改密钥 / 改桶名与端点 / 改授权 / 查网络 /
//! 等恢复）。全部为**明确可重试**的码化错误；失败不影响本地记账（同步是旁路写入）。
//! 文案一律通道无关——后端选谁（WebDAV / S3 兼容对象存储）不该改变用户读到的句子。

pub mod s3;
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

/// 凭据被拒（HTTP 401，或服务端的密钥/签名类错误码）的码化错误单点：明确、
/// 可重试（修正凭据后）。
pub(super) fn auth_failed_error() -> AppError {
    AppError::coded(
        "sync-channel.auth-failed",
        "同步通道凭据被拒绝，请检查通道账号与密钥设置",
    )
}

/// 权限不足（凭据有效但无权访问目标，HTTP 403 / `AccessDenied`）的码化错误
/// 单点：明确、可自救（补授权或换凭据后重试）。
///
/// 与 [`auth_failed_error`] 分成两个码是 issue #1219 的分层要求：S3 对「密钥
/// 不存在 / 签名不符 / 临时凭据过期」也回 403，一律并入「凭据被拒」会让拿着有效
/// 密钥但桶策略没放行的用户收到错误的自救指引（去改密钥），反之亦然。
pub(super) fn permission_denied_error() -> AppError {
    AppError::coded(
        "sync-channel.permission-denied",
        "同步通道权限不足，请检查当前凭据是否具备该桶的读取与写入权限",
    )
}

/// 目标不存在（桶名 / 区域 / 端点指错，HTTP 404 + `NoSuchBucket`）的码化错误
/// 单点：明确、可自救（改配置后重试）。
///
/// 独立成码的必要性：对象存储对「桶不存在」与「对象不存在」都回 404，而后者是
/// 同步增量拉取的常态输入（[`Transport::read_file`] 的 `None`）。不区分会把
/// 「桶名写错」静默降级成「通道上还没有数据」。
pub(super) fn target_missing_error() -> AppError {
    AppError::coded(
        "sync-channel.target-missing",
        "同步通道目标不存在，请检查桶名、区域与端点地址是否正确",
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

/// 非预期 HTTP 状态的码化错误单点（通道服务异常等，可重试）。
pub(super) fn http_failed_error(status: u16, detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.http-failed",
        format!("同步通道服务异常（HTTP {status}）: {detail}"),
        &[&status.to_string(), detail],
    )
}

/// 逻辑通道路径守卫单点：空路径、空段、`.` 与 `..` 一律拒绝（后端共用）。
pub(super) fn validate_logical_path(path: &str) -> crate::error::Result<()> {
    if path.is_empty()
        || path
            .split('/')
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(AppError::codedp(
            "sync-channel.path-invalid",
            format!("同步通道路径非法: {path}"),
            &[path],
        ));
    }
    Ok(())
}
