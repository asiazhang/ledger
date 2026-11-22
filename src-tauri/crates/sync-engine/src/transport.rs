pub mod s3;

use ledger_infra::error::AppError;

pub trait Transport: Send + Sync {
    fn ensure_dir(&self, path: &str) -> ledger_infra::error::Result<()>;

    fn read_file(&self, path: &str) -> ledger_infra::error::Result<Option<Vec<u8>>>;

    fn write_file(&self, path: &str, bytes: &[u8]) -> ledger_infra::error::Result<()>;
}

pub(super) fn auth_failed_error() -> AppError {
    AppError::coded(
        "sync-channel.auth-failed",
        "同步通道凭据被拒绝，请检查通道账号与密钥设置",
    )
}

pub(super) fn permission_denied_error() -> AppError {
    AppError::coded(
        "sync-channel.permission-denied",
        "同步通道权限不足，请检查当前凭据是否具备该桶的读取与写入权限",
    )
}

pub(super) fn target_missing_error() -> AppError {
    AppError::coded(
        "sync-channel.target-missing",
        "同步通道目标不存在，请检查桶名、区域与端点地址是否正确",
    )
}

pub(super) fn network_failed_error(detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.network-failed",
        format!("同步通道网络失败，请检查网络后重试: {detail}"),
        &[detail],
    )
}

pub(super) fn http_failed_error(status: u16, detail: &str) -> AppError {
    AppError::codedp(
        "sync-channel.http-failed",
        format!("同步通道服务异常（HTTP {status}）: {detail}"),
        &[&status.to_string(), detail],
    )
}

pub(super) fn validate_logical_path(path: &str) -> ledger_infra::error::Result<()> {
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
