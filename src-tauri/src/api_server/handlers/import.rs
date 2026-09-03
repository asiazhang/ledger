//! LLM 导入知识端点：导入全流程约定文本（text/plain）单一权威。

use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;

/// LLM 导入知识（纯文本），导入全流程约定的单一权威（拆行、幂等去重、对账、纠错）。
///
/// 本知识是导入约定的确定性权威来源，拆解方式必须固定，否则 `dedup_hash` 漂移。
/// AI 按入口提示词指引自行获取，约定细节不进提示词。
/// 端点契约（请求/响应结构）见 OpenAPI 文档。内容维护在
/// `src-tauri/prompts/import-knowledge.md`，编译期嵌入（`include_str!`）。
const IMPORT_KNOWLEDGE: &str = include_str!("../../../prompts/import-knowledge.md");

#[utoipa::path(
    get,
    path = "/api/v1/import/knowledge",
    tag = "import",
    summary = "获取 LLM 导入知识",
    description = "返回导入全流程约定文本（text/plain），单一权威：每行拆解、商户约定、幂等与去重、\
                  对账完成判定、对账纠错。AI 按入口提示词指引自行获取；文本内嵌 `/api/v1/openapi.json` 地址。",
    responses(
        (status = 200, description = "text/plain 格式的导入知识", content_type = "text/plain", body = String)
    )
)]
pub async fn import_knowledge_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        IMPORT_KNOWLEDGE,
    )
}
