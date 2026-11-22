use crate::market::Market;
use tauri_app_lib::test_support::{extract_check_in_literals, open};

#[test]
fn v002_market_check_literals_match_all() {
    let conn = open();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='instruments'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let literals = extract_check_in_literals(&sql, "market")
        .expect("instruments 表应带 CHECK(market IN (...))");
    let expected: Vec<&str> = Market::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(literals, expected);
}
