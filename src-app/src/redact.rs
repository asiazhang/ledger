//! IPC 日志脱敏的 NSH 对应物（spec #2075 / issue #2077 壳层机制对账②）：载荷
//! 中含主口令/凭据字段时遮蔽其值。语义以根包 `shell_support::redact` 为基准
//!（敏感键闭集、掩码值、null 不遮蔽、对象与数组递归下探逐条一致，不引入新
//! 口径）；根包副本随 #2083 一体切轨退役，本模块是机制在 NSH 的下一任唯一
//! 落点。
//!
//! 本壳现状无 IPC 序列化边界（直调域栈，无载荷日志面），故暂无生产消费面；
//! 模块先行就位，等价脱敏语义随壳走——本壳后续任何会记录调用载荷/敏感字段
//! 的日志与 trace 面（审计、错误上报等）必须经本模块遮蔽后落盘。
//!
//! 语义锚（根包基准，ADR-0075 后果条款 / issue #1217）：
//! - 敏感键闭集：`passphrase` / `new_passphrase` / `newPassphrase` /
//!   `password` / `secret_key` / `secretKey`（S3 Secret Access Key，#1217）；
//! - 掩码值固定 `"••••••"`；
//! - null 不遮蔽（未提供的可选参数原样保留）；
//! - `access_key` 是公开标识（等同账号名），不遮蔽；
//! - 递归下探对象与数组，嵌套结构体入参不漏。

/// 遮蔽敏感字段：按字段名递归匹配（对象与数组皆下探，嵌套结构体入参不漏）。
///
/// 与根包实现逐条对齐：敏感键闭集与掩码值见模块头；null 不遮蔽（未提供的
/// 可选参数原样）；`access_key` 是公开标识（等同账号名），不遮蔽——遮蔽它
/// 会牺牲可观测性而不增加安全性。
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
    /// 嵌套下探是根包 #862 修订的基准语义：结构体入参内层的凭据字段仅看顶层
    /// 会漏进 trace；#1217 起 `config.secret_key`（S3 密钥）同款下探，access_key
    /// 是标识不遮蔽。
    #[test]
    fn sensitive_keys_masked_recursively() {
        let payload: serde_json::Value = serde_json::json!({
            "passphrase": "top-secret",
            "optional": null,
            "config": {
                "endpoint": "https://s3.example.com",
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
        assert_eq!(
            config["endpoint"], "https://s3.example.com",
            "非敏感字段原样"
        );
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

    /// 非对象入参（标量 / 顶层数组 / 空对象）原样透传：脱敏是字段级遮蔽，
    /// 无字段形态的载荷不改动（根包 mask 的 `other` 分支同语义）。
    #[test]
    fn non_object_payloads_pass_through() {
        assert_eq!(
            redact_passphrase_payload(&serde_json::json!("plain text")),
            serde_json::json!("plain text")
        );
        assert_eq!(
            redact_passphrase_payload(&serde_json::json!([1, 2, 3])),
            serde_json::json!([1, 2, 3])
        );
        assert_eq!(
            redact_passphrase_payload(&serde_json::json!({})),
            serde_json::json!({})
        );
    }
}
