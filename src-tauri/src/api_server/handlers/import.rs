//! LLM 导入知识端点：导入全流程约定的单一权威**集合**——知识索引 + 分域知识节
//! （#1123 分级自足上线，ADR-0110）。基础知识与投资节各由一个端点承载，
//! 不保留全文副本（#286 红线：同一节正文只存在一处）。

use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;

/// 基础导入知识（标题 + 知识索引 + 非投资节）：金额与日期口径、每行拆解、商户约定、
/// 个人间借贷、幂等与去重、对账完成判定、对账纠错；知识索引逐节给出分域知识节的
/// 名称、一句话语义与「何时需要读」的触发，是按需取节的唯一陈述点（ADR-0110）。
///
/// 内容维护在 `src-tauri/prompts/import-knowledge-base.md`，编译期嵌入（`include_str!`）。
const BASE_KNOWLEDGE: &str = include_str!("../../../prompts/import-knowledge-base.md");

/// 投资节（投资交易 / 基金申赎 / 基金转换 / 份额调整 / 现金分红）。
///
/// 内容维护在 `src-tauri/prompts/import-knowledge-investment.md`，编译期嵌入
/// （`include_str!`）；投资五节正文只存在于此常量，基础侧不保留副本（#286 红线）。
const INVESTMENT_KNOWLEDGE_SECTIONS: &str =
    include_str!("../../../prompts/import-knowledge-investment.md");

/// 投资五节标题锚点（issue #1185 收敛为单一住处）：#1121 切分边界锚点、
/// 「投资节正文只存在一处」的判定依据，也是 #1123 API 集成锁（基础知识
/// 排他断言 / 投资端点存在断言）的共享锚——两层锁同源，节标题改名只改这里，
/// 漂移在两层断言同步暴露。
/// 生产构建不编译（ADR-0111 决策 5 / #1132 先例）：仅 `cfg(test)`（本模块
/// 单测）与显式启用 `test-utils` feature 的测试构建（集成测试经根包自身
/// dev-dependency 启用）可见，`#[doc(hidden)]` 不进文档。
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

    /// 投资节独有措辞抽查（多腿转换、份额调整、分红三节的确定性表述）：
    /// 整节或其中口径被误搬回基础常量时逐词报红——与端点关键词锁（HTTP 响应层）
    /// 互补，本组断言对准常量结构，守住「同一节正文只存在一处」。
    const INVESTMENT_ONLY_WORDING: [&str; 7] = [
        "tradingTarget",
        "convertAmount",
        "逐腿直读",
        "尾差末腿",
        "带符号份额增量",
        "红利再投",
        "到账账户",
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

    /// 分级自足上线（#1123 / ADR-0110）：#1121 的占位拼接退役，两常量即两端点
    /// 响应——任何占位标记不得残留；知识索引段与投资端点指针必须在基础侧在位。
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
