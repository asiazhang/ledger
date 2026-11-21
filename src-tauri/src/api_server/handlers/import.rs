use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;

const BASE_KNOWLEDGE: &str = include_str!("../../../prompts/import-knowledge-base.md");

const INVESTMENT_KNOWLEDGE_SECTIONS: &str =
    include_str!("../../../prompts/import-knowledge-investment.md");

#[cfg(any(test, feature = "test-utils"))]
#[doc(hidden)]
pub const INVESTMENT_SECTION_HEADERS: [&str; 5] = [
    "## 投资交易（buy / sell）",
    "## 基金申赎（buy / sell，场外基金）",
    "## 基金转换（convert，场外基金）",
    "## 份额调整（split，场外基金与股票同构）",
    "## 现金分红（dividend，股息 / 基金分红 / 投顾组合分红同构）",
];

#[utoipa::path(
    get,
    path = "/api/v1/import/knowledge",
    tag = "import",
    summary = "获取 LLM 导入基础知识",
    description = "返回导入基础知识（text/plain）：知识索引 + 非投资节——每行拆解、商户约定、个人间借贷、\
                  幂等与去重、对账完成判定、对账纠错；分域知识节按知识索引指引按需获取（投资五节见 \
                  GET /api/v1/import/knowledge/investment）。AI 按入口提示词指引自行获取；\
                  文本内嵌 `/api/v1/contract`（紧凑契约方言）地址。",
    responses(
        (status = 200, description = "text/plain 格式的导入基础知识", content_type = "text/plain", body = String)
    )
)]
pub async fn import_knowledge_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        BASE_KNOWLEDGE,
    )
}

#[utoipa::path(
    get,
    path = "/api/v1/import/knowledge/investment",
    tag = "import",
    summary = "获取 LLM 导入投资知识",
    description = "返回导入知识的投资节全文（text/plain）：投资交易（buy/sell）、基金申赎、基金转换、\
                  份额调整、现金分红——标的解析、投资落账与投资的对账、纠错口径。按基础知识的知识索引\
                  指引按需获取：AI 记账会话多数行与投资无关，出现投资类流水或标的时才读本端点。",
    responses(
        (status = 200, description = "text/plain 格式的投资节知识", content_type = "text/plain", body = String)
    )
)]
pub async fn import_investment_knowledge_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        INVESTMENT_KNOWLEDGE_SECTIONS,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVESTMENT_ONLY_WORDING: [&str; 11] = [
        "tradingTarget",
        "convertAmount",
        "逐腿直读",
        "尾差末腿",
        "带符号份额增量",
        "红利再投",
        "到账账户",
        "共五个 kind 值",
        "一律不带商户",
        "entity_id",
        "display_name",
    ];

    #[test]
    fn investment_sections_live_only_in_investment_constant() {
        for header in INVESTMENT_SECTION_HEADERS {
            assert!(
                INVESTMENT_KNOWLEDGE_SECTIONS.contains(header),
                "投资节常量应包含 {header}"
            );
            assert!(
                !BASE_KNOWLEDGE.contains(header),
                "基础常量不得包含投资节正文 {header}（不引入重复正文）"
            );
        }
        for kw in INVESTMENT_ONLY_WORDING {
            assert!(
                !BASE_KNOWLEDGE.contains(kw),
                "基础常量不得含投资节独有措辞 {kw:?}"
            );
        }
    }

    #[test]
    fn base_knowledge_carries_index_and_constants_carry_no_placeholder() {
        assert!(!BASE_KNOWLEDGE.contains("{{"), "基础常量不得含占位标记");
        assert!(
            !INVESTMENT_KNOWLEDGE_SECTIONS.contains("{{"),
            "投资节常量不得含占位标记"
        );
        assert!(
            BASE_KNOWLEDGE.contains("## 知识索引"),
            "基础常量应含知识索引段（分域知识节的目录与触发单点）"
        );
        assert!(
            BASE_KNOWLEDGE.contains("GET /api/v1/import/knowledge/investment"),
            "知识索引应带投资节端点指针"
        );
    }
}
