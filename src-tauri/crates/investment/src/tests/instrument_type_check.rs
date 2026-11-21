use crate::InstrumentType;
use tauri_app_lib::test_support::{extract_check_in_literals, open};

#[test]
fn v002_instrument_type_check_literals_match_all() {
    let conn = open();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='instruments'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let literals = extract_check_in_literals(&sql, "instrument_type")
        .expect("instruments 表应带 CHECK(instrument_type IN (...))");
    let expected: Vec<&str> = InstrumentType::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(literals, expected);
}
