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
        assert_eq!(payload["passphrase"], "top-secret");
    }
}
