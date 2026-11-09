//! wire 入参「键缺席 vs 值为 `null`」三态区分器的全仓唯一住址（issue #1330 自
//! 账户域与分类域的两份同源拷贝收敛于此，ADR-0111 决策 2 原语区顶层单文件）。
//!
//! 背景：更新入参的「键缺席 = 不改、值为 `null` = 清空」三态语义，serde 默认
//! 给不出来——`Option<Option<T>>` 会把「键缺席」与「值为 `null`」折叠成同一个
//! `None`（`null` 走 `Option` 的 `visit_unit`），不分开则「清空」在 wire 上
//! 不可达，必须显式 `deserialize_with`。两个字段注解缺一不可：
//!
//! - `#[serde(default)]`：键缺席时不调用区分器、直接交回 `None`（= 不改）；
//! - `deserialize_with = "ledger_infra::serde_util::double_option"`：键在场时
//!   被调用，`null` → `Some(None)`（= 清空）、给值 → `Some(Some(v))`（= 落定
//!   该值）。
//!
//! 消费方：账户域信用卡档案三字段（spec #1327）、分类域 icon / parent_id
//! （#1327 范围外修复）。纯 serde 机制、不定义任何账本数据口径，按决策 1
//! 「跨层共享机制」归基础设施。

/// 「键缺席 = 不改」与「值为 `null` = 清空」的反序列化区分器（用法见模块文档）：
/// 仅在键在场时被调用（缺席由 `#[serde(default)]` 交回 `None`），因此「键在场
/// 即 `Some`」恰好等价于三态里的后两态。
pub fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    use serde::Deserialize as _;
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::double_option;
    use serde::Deserialize;

    /// 探针与生产调用点同形：`#[serde(default)]` + `deserialize_with` 成对出现。
    #[derive(Deserialize)]
    struct StringProbe {
        #[serde(default, deserialize_with = "double_option")]
        field: Option<Option<String>>,
    }

    #[derive(Deserialize)]
    struct IntProbe {
        #[serde(default, deserialize_with = "double_option")]
        field: Option<Option<i64>>,
    }

    /// 三态逐一钉住：显式 `null` = 清空（`Some(None)`）、键缺席 = 不改（`None`）、
    /// 给值 = 落定（`Some(Some(v))`）。删掉任一注解即红——`default` 缺席则键缺席
    /// 报 missing field，`deserialize_with` 缺席则 `null` 折叠成「不改」。
    #[test]
    fn double_option_treats_null_as_clear_not_no_change() {
        let cleared: StringProbe = serde_json::from_str(r#"{"field":null}"#).unwrap();
        assert_eq!(cleared.field, Some(None), "显式 null = 清空");
        let untouched: StringProbe = serde_json::from_str("{}").unwrap();
        assert_eq!(untouched.field, None, "键缺席 = 不改");
        let set: StringProbe = serde_json::from_str(r#"{"field":"🍕"}"#).unwrap();
        assert_eq!(set.field, Some(Some("🍕".into())), "给值 = 落定");
    }

    /// 区分器泛型于载荷类型：非 `String` 形（账户域信用卡档案的 `i64` 形）同款三态。
    #[test]
    fn double_option_is_generic_over_payload_type() {
        let cleared: IntProbe = serde_json::from_str(r#"{"field":null}"#).unwrap();
        assert_eq!(cleared.field, Some(None), "显式 null = 清空");
        let untouched: IntProbe = serde_json::from_str("{}").unwrap();
        assert_eq!(untouched.field, None, "键缺席 = 不改");
        let set: IntProbe = serde_json::from_str(r#"{"field":5000}"#).unwrap();
        assert_eq!(set.field, Some(Some(5000)), "给值 = 落定");
    }
}
