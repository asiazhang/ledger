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

    #[test]
    fn double_option_treats_null_as_clear_not_no_change() {
        let cleared: StringProbe = serde_json::from_str(r#"{"field":null}"#).unwrap();
        assert_eq!(cleared.field, Some(None), "显式 null = 清空");
        let untouched: StringProbe = serde_json::from_str("{}").unwrap();
        assert_eq!(untouched.field, None, "键缺席 = 不改");
        let set: StringProbe = serde_json::from_str(r#"{"field":"🍕"}"#).unwrap();
        assert_eq!(set.field, Some(Some("🍕".into())), "给值 = 落定");
    }

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
