/// AI 入口提示词模板，供用户复制给 AI 编程助手（如 Cursor、Claude Code）使用。
/// 与 `GET /api/v1/import/knowledge`（导入知识）分界：本提示词只是入口骨架
/// （是什么 + 基址与常驻排查 + 三步 + 「不删后重导」纪律），约定细节的单一
/// 权威在导入知识与 OpenAPI 契约，AI 按指引自行获取。
const AI_PROMPT: &str = include_str!("../../prompts/ledger-api.md");

/// 获取 AI 入口提示词全文（markdown 原文）。
///
/// 保持同步形态（形状乙 sweep 判定，spec #498 / #503）：纯内存常量克隆
///（`include_str!`，无 DB、无 IO、无阻塞工作面），与「全部触碰 DB 的命令
/// async 化」口径一致（先例：`set_auto_execution_enabled`、`cancel_sync_instruments`）。
#[tauri::command]
pub fn get_ai_prompt() -> String {
    AI_PROMPT.to_string()
}

#[cfg(test)]
mod tests {
    use super::AI_PROMPT;

    /// 入口提示词骨架契约锁（issue #286，骨架定义见 AI_PROMPT 常量注释）：
    /// 三步与纪律在位、约定细节零复述——防细节回流造成两处口径分叉。
    #[test]
    fn prompt_is_entry_skeleton_without_convention_details() {
        // 三步入口与唯一纪律在位
        assert!(
            AI_PROMPT.contains("GET /api/v1/openapi.json"),
            "应指引发现阶段：契约发现"
        );
        assert!(
            AI_PROMPT.contains("GET /api/v1/import/knowledge"),
            "应指引发现阶段：获取导入知识"
        );
        assert!(AI_PROMPT.contains("不删后重导"), "应保留唯一入口级纪律");
        // 服务常驻排查提示在位（连接失败 → 请用户启动开源记账后重试；标题
        // 「Ledger API」是协议契约不做断言，正文用户指引随显示名更新 ADR-0076）
        assert!(AI_PROMPT.contains("启动开源记账"), "应包含服务常驻排查提示");
        // 不复述细节：幂等键格式、商户建议、对账细则、纠错与错误响应、废弃别名
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
