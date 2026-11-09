//! 统一错误响应格式：`{kind, message[, code, params]}` JSON。
//!
//! `AppError` → HTTP 状态码的投影实现随错误类型住基础设施 crate（issue #1088：
//! 孤儿规则要求 trait 实现与类型同 crate，`ledger_infra::error` 提供
//! `impl IntoResponse for AppError`）；本模块只保留响应 DTO 的形状声明。

use utoipa::ToSchema;

/// 统一错误响应格式：`{ "kind": "<ErrorKind>", "message": "<中文描述>" }`；
/// 码化错误额外携带稳定 `code` 与可选 `params`（issue #342 二期 / ADR-0050，只增不改）。
#[derive(ToSchema)]
#[allow(dead_code)]
pub struct ErrorResponse {
    /// 错误类型枚举：`Db` / `NotFound` / `Invalid` / `Parse` / `Io`
    kind: String,
    /// 中文错误描述
    message: String,
    /// 稳定错误码（可选，仅码化错误与系统类错误携带），领域语言命名如 `transfer.to-account-required`
    code: Option<String>,
    /// 插值参数（可选，按消息中动态值出现顺序）
    params: Option<Vec<String>>,
}
