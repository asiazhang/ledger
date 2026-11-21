use ledger_infra::db::query::query_all;
use ledger_infra::error::AppError;

use super::model::Currency;
use tauri_app_lib::test_support::{assert_op_write_failure, block_op_writes};

fn setup() -> rusqlite::Connection {
    tauri_app_lib::test_support::open()
}

#[test]
fn list_currencies_returns_all_seed_currencies() {
    let conn = setup();
    let currencies: Vec<Currency> = query_all(
        &conn,
        "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
        [],
    )
    .unwrap();
    assert_eq!(currencies.len(), 11);
    assert!(currencies.iter().any(|c| c.code == "CNY"));
    assert!(currencies.iter().any(|c| c.code == "USD"));
    assert!(currencies.iter().any(|c| c.code == "EUR"));
    assert!(currencies.iter().any(|c| c.code == "HKD"));
}

#[test]
fn currencies_have_correct_decimal_places() {
    let conn = setup();
    let currencies: Vec<Currency> = query_all(
        &conn,
        "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
        [],
    )
    .unwrap();
    for c in &currencies {
        assert!(
            c.decimal_places >= 0,
            "{} decimal_places is negative",
            c.code
        );
    }
    let cny = currencies.iter().find(|c| c.code == "CNY").unwrap();
    assert_eq!(cny.decimal_places, 2);
    let usd = currencies.iter().find(|c| c.code == "USD").unwrap();
    assert_eq!(usd.decimal_places, 2);
}

#[test]
fn base_currency_defaults_to_cny_when_unset() {
    let conn = setup();
    assert_eq!(
        super::base_currency::current_base_currency(&conn).unwrap(),
        "CNY"
    );
}

#[test]
fn set_base_currency_roundtrips() {
    let conn = setup();
    super::base_currency::set_base_currency(&conn, "USD").unwrap();
    assert_eq!(
        super::base_currency::current_base_currency(&conn).unwrap(),
        "USD"
    );
}

#[test]
fn set_base_currency_rejects_unknown_code() {
    let conn = setup();
    let err = super::base_currency::set_base_currency(&conn, "XXY").unwrap_err();
    assert!(
        matches!(err, AppError::Coded { ref code, .. } if code == "currency.base-invalid"),
        "应为码化错误 currency.base-invalid，实际 {err:?}"
    );
    assert_eq!(
        super::base_currency::current_base_currency(&conn).unwrap(),
        "CNY"
    );
}

#[test]
fn set_base_currency_op_failure_rolls_back_setting() {
    let conn = setup();
    block_op_writes(&conn);

    let err = super::base_currency::set_base_currency(&conn, "USD").unwrap_err();
    assert_op_write_failure(&err);

    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key='ledger.base_currency'",
            [],
            |r| r.get(0),
        )
        .ok();
    assert!(
        stored.is_none(),
        "op 落库失败时设置行必须随事务回滚，不得残留，实际 {stored:?}"
    );
}
