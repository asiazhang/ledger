const AI_PROMPT: &str = include_str!("../../prompts/ledger-api.md");

#[tauri::command]
pub fn get_ai_prompt() -> String {
    AI_PROMPT.to_string()
}

#[cfg(test)]
mod tests {
    use super::AI_PROMPT;

    #[test]
    fn prompt_is_entry_skeleton_without_convention_details() {
        assert!(
            AI_PROMPT.contains("GET /api/v1/contract"),
            "应指引发现阶段：契约发现（紧凑方言）"
        );
        assert!(
            !AI_PROMPT.contains("openapi.json"),
            "旧契约发现地址应退役（issue #839 路径换轨）"
        );
        assert!(
            AI_PROMPT.contains("GET /api/v1/import/knowledge"),
            "应指引发现阶段：获取导入知识"
        );
        assert!(AI_PROMPT.contains("AI 记账"), "应承认会话形态：AI 记账");
        assert!(AI_PROMPT.contains("数据迁移"), "应承认会话形态：数据迁移");
        assert!(
            !AI_PROMPT.contains("主要场景"),
            "迁移为主的措辞应退役（issue #1122）"
        );
        assert!(
            !AI_PROMPT.contains("也可直接录入记账"),
            "直录为辅的措辞应退役（issue #1122）"
        );
        assert!(AI_PROMPT.contains("不删后重导"), "应保留唯一入口级纪律");
        assert!(AI_PROMPT.contains("启动开源记账"), "应包含服务常驻排查提示");
        for banned in [
            "idempotency_key",
            "merchant_name",
            "merchants",
            "dedup",
            "sha256",
            "involving_account_id",
            "balances",
            "PUT /api/v1",
            "DELETE /api/v1",
            "对账不过",
            "错误响应",
            "系统提示词",
        ] {
            assert!(
                !AI_PROMPT.contains(banned),
                "入口提示词不应复述细节（卸载至导入知识/契约）：{banned}"
            );
        }
    }
}
