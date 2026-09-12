//! LLM 导入知识端点：导入全流程约定文本（text/plain）单一权威。

use std::sync::LazyLock;

use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;

/// 基础导入知识（标题 + 非投资节）：金额与日期口径、每行拆解、商户约定、个人间借贷、
/// 幂等与去重、对账完成判定、对账纠错。投资五节的拼接位置以 [`INVESTMENT_SECTIONS_SLOT`]
/// 占位标记指示，完整知识由基础 + 投资节拼回（见 [`FULL_IMPORT_KNOWLEDGE`]；
/// #1121 前置搬家，ADR-0110）。
///
/// 内容维护在 `src-tauri/prompts/import-knowledge-base.md`，编译期嵌入（`include_str!`）。
const BASE_KNOWLEDGE: &str = include_str!("../../../prompts/import-knowledge-base.md");

/// 投资节（投资交易 / 基金申赎 / 基金转换 / 份额调整 / 现金分红）。
///
/// 内容维护在 `src-tauri/prompts/import-knowledge-investment.md`，编译期嵌入
/// （`include_str!`）；投资五节正文只存在于此常量，基础侧不保留副本（#286 红线）。
const INVESTMENT_KNOWLEDGE_SECTIONS: &str =
    include_str!("../../../prompts/import-knowledge-investment.md");

/// 基础知识中投资五节的占位标记行（标记 + 换行）：
/// [`FULL_IMPORT_KNOWLEDGE`] 在此精确一次替换为 [`INVESTMENT_KNOWLEDGE_SECTIONS`]。
const INVESTMENT_SECTIONS_SLOT: &str = "{{IMPORT_KNOWLEDGE_INVESTMENT_SECTIONS}}\n";

/// 完整导入知识 = 基础知识 + 投资节（占位处拼回），首次访问时组合一次。
///
/// 行为不变阶段（#1121）：原知识端点仍返回完整知识；分级自足上线（#1123）后
/// 两个知识端点分别返回 [`BASE_KNOWLEDGE`] 与 [`INVESTMENT_KNOWLEDGE_SECTIONS`]，
/// 届时删除本组合、不留全文副本。
static FULL_IMPORT_KNOWLEDGE: LazyLock<String> = LazyLock::new(|| {
    BASE_KNOWLEDGE.replacen(INVESTMENT_SECTIONS_SLOT, INVESTMENT_KNOWLEDGE_SECTIONS, 1)
});

#[utoipa::path(
    get,
    path = "/api/v1/import/knowledge",
    tag = "import",
    summary = "获取 LLM 导入知识",
    description = "返回导入全流程约定文本（text/plain），单一权威：每行拆解、商户约定、幂等与去重、\
                  对账完成判定、对账纠错。AI 按入口提示词指引自行获取；文本内嵌 `/api/v1/contract`（紧凑契约方言）地址。",
    responses(
        (status = 200, description = "text/plain 格式的导入知识", content_type = "text/plain", body = String)
    )
)]
pub async fn import_knowledge_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        FULL_IMPORT_KNOWLEDGE.as_str(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 投资五节标题：#1121 切分边界锚点，也是「投资节正文只存在一处」的判定依据。
    const INVESTMENT_SECTION_HEADERS: [&str; 5] = [
        "## 投资交易（buy / sell）",
        "## 基金申赎（buy / sell，场外基金）",
        "## 基金转换（convert，场外基金）",
        "## 份额调整（split，场外基金与股票同构）",
        "## 现金分红（dividend，股息 / 基金分红 / 投顾组合分红同构）",
    ];

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

    #[test]
    fn base_knowledge_carries_exactly_one_slot_line() {
        assert_eq!(
            BASE_KNOWLEDGE.matches(INVESTMENT_SECTIONS_SLOT).count(),
            1,
            "基础常量应恰好含一个投资节占位标记行"
        );
        assert!(
            !INVESTMENT_KNOWLEDGE_SECTIONS.contains("{{"),
            "投资节常量不得含占位标记"
        );
    }

    #[test]
    fn full_knowledge_splices_investment_sections_without_marker_leftover() {
        let full = &*FULL_IMPORT_KNOWLEDGE;
        assert!(!full.contains("{{"), "拼接后的完整知识不得残留占位标记");
        for header in INVESTMENT_SECTION_HEADERS {
            assert_eq!(
                full.matches(header).count(),
                1,
                "完整知识应恰好含一次 {header}"
            );
        }
    }
}
