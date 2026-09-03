//! 统一错误响应：`AppError` → HTTP 状态码 + `{kind, message[, code, params]}` JSON。

use crate::error::{AppError, ErrClass};
use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use utoipa::ToSchema;

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self {
            AppError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Invalid(_) => StatusCode::BAD_REQUEST,
            AppError::Parse(_) => StatusCode::BAD_REQUEST,
            AppError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
            // 码化错误按归类取状态（ADR-0050）：Invalid→400、NotFound→404
            AppError::Coded { class, .. } => match class {
                ErrClass::Invalid => StatusCode::BAD_REQUEST,
                ErrClass::NotFound => StatusCode::NOT_FOUND,
            },
        };
        (status, Json(self)).into_response()
    }
}

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
