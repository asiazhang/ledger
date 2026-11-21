use tauri_app_lib::ledger_transaction::TransactionKind;
use tauri_app_lib::test_support::{extract_check_in_literals, open};

#[test]
fn v001_kind_check_literals_match_all() {
    let conn = open();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='transactions'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let literals =
        extract_check_in_literals(&sql, "kind").expect("transactions 表应带 CHECK(kind IN (...))");
    let expected: Vec<&str> = TransactionKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(literals, expected);
}
