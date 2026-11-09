//! 时间与身份工厂（issue #1128 / ADR-0111 决策 2 原语区顶层单文件）：
//! 当前时刻、ISO 格式、UUID v7 生成与 UUID v5 确定性派生的全仓唯一住址。
//! 它不是数据库关切——文件工具等原语引用本模块，不穿透 `db/`。
//!
//! 既有调用点经 `db` 模块再导出保持可用（根包 `crate::db::…` 再导出面与
//! 协议 crate 的 `ledger_infra::db::…` 路径零改动）。
//!
//! `new_uuid` 的取值契约（issue #1489）：v7 **进程内严格单调**——同一毫秒内
//! 连续生成也严格递增。读路径普遍把「主键时间有序」当同日 / 同刻的插入序破平键
//!（各域口径见对应词汇表），这条不变量是那些口径成立的前提，不得回退为同毫秒内
//! 低位随机的形态。

/// 当前 UTC 时间 ISO 字符串。
pub fn now_iso() -> String {
    iso_at(chrono::Utc::now())
}

/// 把注入的时刻格式化为与 [`now_iso`] 同格式的 UTC ISO 字符串。
/// 供需注入时钟的调用方（如自动备份锚点）使用，保证全仓唯一格式定义。
pub fn iso_at(now: chrono::DateTime<chrono::Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 生成新的 UUID v7（适合主键与同步）。
///
/// **进程内严格单调**（issue #1489）：经 uuid crate 的共享 `ContextV7` 计数器，
/// 同一毫秒内连续生成也严格递增，故本进程内「id 升序 = 插入序」。`NoContext`
/// 会让同毫秒内的低位随机、id 序与插入序脱钩——下游按 id 取最新 / 破平的读
/// 口径会读到陈旧行（实物资产详情估值调度敏感缺陷的根因）。跨端落地的行不受
/// 本进程约束：同步重放沿用来源端 id，同日跨端先后仍按来源端时间序。
pub fn new_uuid() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// 确定性 UUID v5 的本仓命名空间（跨端一致派生 id 的派生根；先例：V004 默认
/// 种子的确定性 UUID v5——同名恒同值，保证各端独立派生不产生重复行）。
pub const DETERMINISTIC_NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes(*b"ledger_sync_v5_1");

/// 确定性 UUID v5：同名同空间跨端恒同值（同步场景的确定性落地身份）。
pub fn deterministic_uuid(name: &str) -> String {
    uuid::Uuid::new_v5(&DETERMINISTIC_NAMESPACE, name.as_bytes()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ISO 格式逐字节钉住：与迁移种子 `strftime('%Y-%m-%dT%H:%M:%SZ','now')`
    /// 同格式（V004 头注），升模块不得改变任何字节的输出。
    #[test]
    fn iso_at_pins_byte_identical_format() {
        let t = chrono::DateTime::parse_from_rfc3339("2026-03-01T12:34:56Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(iso_at(t), "2026-03-01T12:34:56Z");
    }

    /// 确定性 v5 逐字节钉住（黄金值独立于 Rust 实现预先算出，SHA-1 派生
    /// 跨端恒同）：同名恒同值、异名异值、版本号位恒为 5。
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

    /// v7 生成：版本号位恒为 7，两次调用产出不同值。
    #[test]
    fn new_uuid_generates_version7_unique_values() {
        let a = new_uuid();
        let b = new_uuid();
        assert_eq!(a.as_bytes()[14], b'7');
        assert_ne!(a, b);
    }

    /// v7 生成**进程内严格单调**（issue #1489）：同一毫秒内连续生成也不得回退。
    /// 下游读路径以「id 升序 = 插入序」当同日破平键，回退即读到陈旧行——本单测
    /// 是这条不变量的确定性守门（删修复即变红）。
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
