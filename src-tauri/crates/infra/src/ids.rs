pub fn now_iso() -> String {
    iso_at(chrono::Utc::now())
}

pub fn iso_at(now: chrono::DateTime<chrono::Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn new_uuid() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub const DETERMINISTIC_NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes(*b"ledger_sync_v5_1");

pub fn deterministic_uuid(name: &str) -> String {
    uuid::Uuid::new_v5(&DETERMINISTIC_NAMESPACE, name.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_at_pins_byte_identical_format() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-03-01T12:34:56Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(iso_at(t), "2026-03-01T12:34:56Z");
    }

    #[test]
    fn deterministic_uuid_pins_byte_identical_derivation() {
        assert_eq!(
            deterministic_uuid("ledger-ids-probe"),
            "04241597-acb0-5f01-ac19-6de0ee9a159f"
        );
        assert_eq!(
            deterministic_uuid("餐饮"),
            "5e80d32e-cf4a-5bdb-adc3-fcc805b8e709"
        );
        assert_ne!(
            deterministic_uuid("ledger-ids-probe"),
            deterministic_uuid("other")
        );
        assert_eq!(deterministic_uuid("ledger-ids-probe").as_bytes()[14], b'5');
    }

    #[test]
    fn new_uuid_generates_version7_unique_values() {
        let a = new_uuid();
        let b = new_uuid();
        assert_eq!(a.as_bytes()[14], b'7');
        assert_ne!(a, b);
    }

    #[test]
    fn new_uuid_is_process_monotonic() {
        let mut previous = new_uuid();
        for _ in 0..1024 {
            let next = new_uuid();
            assert!(
                next > previous,
                "uuid v7 应进程内严格单调：{previous} 不应大于等于 {next}"
            );
            previous = next;
        }
    }
}
