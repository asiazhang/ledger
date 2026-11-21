use crate::InstrumentType;

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

#[test]
fn instrument_type_parse_roundtrip() {
    for kind in InstrumentType::ALL {
        assert_eq!(InstrumentType::parse(kind.as_str()).unwrap(), kind);
        assert_eq!(kind.to_string(), kind.as_str());
    }
    let err = InstrumentType::parse("etfs").unwrap_err();
    assert_eq!(err.code(), Some("instrument.type-unknown"));
    assert!(err.to_string().contains("未知金融工具类型"), "实际: {err}");
    assert!(
        err.to_string().contains("stock/fund/bond/etf/other"),
        "实际: {err}"
    );
    assert_eq!(
        serde_json::to_value(&err).unwrap()["params"],
        serde_json::json!(["etfs", "stock/fund/bond/etf/other"]),
        "params 按动态值出现顺序：未知值 → 同源合法值清单"
    );
}
