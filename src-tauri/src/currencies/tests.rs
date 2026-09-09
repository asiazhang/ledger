use crate::db::query::query_all;
use crate::error::AppError;

use super::model::Currency;

fn setup() -> rusqlite::Connection {
    // 建库两行序经统一测试工厂承载（spec #728 / issue #754 / ADR-0084 决策 7）。
    crate::test_support::open()
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

// ── 本位币基准（LedgerLevelSetting 首个成员，issue #858 / ADR-0091 决策 3）──
// 存储落 AppSettings（ADR-0017：后端消费配置存库），读写收口本域。

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
    // 校验失败不落库：基准保持默认。
    assert_eq!(
        super::base_currency::current_base_currency(&conn).unwrap(),
        "CNY"
    );
}
