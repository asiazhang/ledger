//! `InstrumentType` 字符串面（ADR-0108 宏同体派生）：serde wire 形状、
//! `parse` 严格映射与未知值报错文案。
//!
//! wire 形状锁死为小写字符串（与 V002 `instrument_type` CHECK、既有
//! `rename_all = "snake_case"` 的 JSON 形状逐字一致）；未知值文案与
//! `parse` 同源（中文 + 合法值清单）。

use crate::investment::InstrumentType;

/// serde 以小写字符串序列化（wire 兼容：与 `rename_all = "snake_case"`
/// 时代的 JSON 形状一致），反序列化严格往返；未知值报错文案与 parse 同源。
#[test]
fn instrument_type_serde_roundtrip() {
    for kind in InstrumentType::ALL {
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, format!("\"{}\"", kind.as_str()));
        let back: InstrumentType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }
    let err = serde_json::from_str::<InstrumentType>("\"etfs\"").unwrap_err();
    assert!(err.to_string().contains("未知金融工具类型"), "实际: {err}");
}

/// `Display` / `parse` 严格往返；未知值报参数错误并附合法值清单。
#[test]
fn instrument_type_parse_roundtrip() {
    for kind in InstrumentType::ALL {
        assert_eq!(InstrumentType::parse(kind.as_str()).unwrap(), kind);
        assert_eq!(kind.to_string(), kind.as_str());
    }
    let err = InstrumentType::parse("etfs").unwrap_err();
    assert!(err.to_string().contains("未知金融工具类型"), "实际: {err}");
    assert!(
        err.to_string().contains("stock/fund/bond/etf/other"),
        "实际: {err}"
    );
}
