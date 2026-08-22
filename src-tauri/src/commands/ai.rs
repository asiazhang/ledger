/// AI 入口系统提示词模板，供用户复制给 AI 编程助手（如 Cursor、Claude Code）使用。
/// 内容与 `GET /api/v1/import/knowledge`（导入知识）互补：本提示词是入口，
/// 指引 AI 自行通过 HTTP API 发现端点并获取导入约定，完成迁移闭环。
const AI_PROMPT: &str = include_str!("../../prompts/ledger-api.md");

/// 获取 AI 入口系统提示词全文（markdown 原文）。
#[tauri::command]
pub fn get_ai_prompt() -> String {
    AI_PROMPT.to_string()
}
