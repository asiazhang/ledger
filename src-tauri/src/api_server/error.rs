use utoipa::ToSchema;

#[derive(ToSchema)]
#[schema(
    description = "统一错误响应格式：`{ \"kind\": \"<ErrorKind>\", \"message\": \"<中文描述>\" }`；码化错误额外携带稳定 `code` 与可选 `params`（issue #342 二期 / ADR-0050，只增不改）。"
)]
#[allow(dead_code)]
pub struct ErrorResponse {
    #[doc = "错误类型枚举：`Db` / `NotFound` / `Invalid` / `Parse` / `Io`"]
    kind: String,
    #[doc = "中文错误描述"]
    message: String,
    #[doc = "稳定错误码（可选，仅码化错误与系统类错误携带），领域语言命名如 `transfer.to-account-required`"]
    code: Option<String>,
    #[doc = "插值参数（可选，按消息中动态值出现顺序）"]
    params: Option<Vec<String>>,
}
