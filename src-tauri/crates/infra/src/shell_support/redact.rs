//! IPC 日志脱敏：载荷中含主口令字段时遮蔽其值。
//!
//! 审计日志与 trace 输出不落主口令（ADR-0075 后果条款）：载荷任一深度出现
//! 敏感字段即遮蔽其值。原为根包 `lib.rs` 的私有函数，随 workspace 骨架
//!（spec #1086 / issue #1087）迁入基础设施 crate；#1130 起收进 [`crate::shell_support`]
//! 暂住分组——壳机制暂住：正住址是壳层（IPC 日志接线是壳层调用点），
//! 随 #1086 P5 壳层收敛迁出，不得作为基础设施范式被引用（ADR-0111 决策 2）。

/// 遮蔽敏感字段：按字段名递归匹配（对象与数组皆下探，嵌套结构体入参不漏）。
///
/// 主口令/凭据字段永不落日志/trace（ADR-0075）：解锁/开启加密的 `passphrase`
/// 与修改主口令的 `new_passphrase` 同等敏感（Tauri v2 参数名按 JS 侧
/// camelCase 到达，两种拼法都遮蔽）；`password` 覆盖嵌套结构体形态的凭据字段
/// （issue #862 同步通道配置，WebDAV 密码与主口令同级处置）；`secret_key` 是
/// S3 兼容对象存储的 Secret Access Key（issue #1217，与主口令同级处置）——
/// `access_key` 是公开标识（等同账号名），不遮蔽，遮蔽它会牺牲可观测性而不
/// 增加安全性。
pub fn redact_passphrase_payload(payload: &serde_json::Value) -> serde_json::Value {
    const SENSITIVE_KEYS: &[&str] = &[
        "passphrase",
        "new_passphrase",
        "newPassphrase",
        "password",
        "secret_key",
        "secretKey",
    ];
    fn mask(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.iter()
                    .map(|(key, val)| {
                        let masked = if SENSITIVE_KEYS.contains(&key.as_str()) && !val.is_null() {
                            serde_json::Value::String("••••••".into())
                        } else {
                            mask(val)
                        };
                        (key.clone(), masked)
                    })
                    .collect(),
            ),
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(mask).collect())
            }
            other => other.clone(),
        }
    }
    mask(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 敏感键遮蔽（ADR-0075）：顶层与嵌套形态的 passphrase/password/secret_key
    /// 都被遮蔽，null 不遮蔽（未提供的可选参数原样保留），非敏感字段原样透传。
    /// 嵌套下探是 #862 修订：通道配置的 `config.password` 在结构体入参内层，
    /// 仅看顶层会漏进 trace；#1217 起 `config.secret_key`（S3 密钥）同款下探，
    /// access_key 是标识不遮蔽。
    #[test]
    fn sensitive_keys_masked_recursively() {
        let payload: serde_json::Value = serde_json::json!({
            "passphrase": "top-secret",
            "optional": null,
            "config": {
                "base_url": "https://dav.example.com/dav/",
                "username": "alice",
                "password": "app-pass",
                "access_key": "AKIAEXAMPLE",
                "secret_key": "s3-secret",
                "note": null,
                "deep": [{ "newPassphrase": "inner" }, { "secretKey": "inner-camel" }],
            },
        });
        let masked = redact_passphrase_payload(&payload);
        let obj = masked.as_object().unwrap();
        assert_eq!(obj["passphrase"], "••••••");
        assert_eq!(obj["optional"], serde_json::Value::Null, "null 不遮蔽");
        let config = obj["config"].as_object().unwrap();
        assert_eq!(config["password"], "••••••");
        assert_eq!(config["username"], "alice", "非敏感字段原样");
        assert_eq!(config["secret_key"], "••••••", "S3 密钥遮蔽");
        assert_eq!(
            config["access_key"], "AKIAEXAMPLE",
            "access key 是标识，不遮蔽"
        );
        assert_eq!(config["note"], serde_json::Value::Null, "null 不遮蔽");
        assert_eq!(config["deep"][0]["newPassphrase"], "••••••", "数组内也下探");
        assert_eq!(
            config["deep"][1]["secretKey"], "••••••",
            "camelCase 变体同样遮蔽"
        );
        // 原载荷不被改动。
        assert_eq!(payload["passphrase"], "top-secret");
    }
}
