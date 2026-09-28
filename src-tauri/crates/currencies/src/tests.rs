use ledger_infra::db::query::query_all;
use ledger_infra::error::AppError;

use super::model::Currency;

fn setup() -> rusqlite::Connection {
    // 建库两行序经统一测试工厂承载（spec #728 / issue #754 / ADR-0084 决策 7）；
    // 工厂住根包，经 dev-dependency 测试环消费（#1095，ledger-transaction 同款）。
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

/// op 落库失败时设置行必须随事务回滚（issue #1867，ADR-0139 决策 2 的先行
/// 缺陷票）：「校验 → 落库 → op 追加」同生共死。失败注入用纯测试侧手段
/// （既有先例：`db::tx_scope` 行为单测的触发器 RAISE(ABORT)）——
/// `BEFORE INSERT ON sync_ops` 触发器挡下 op 落库，使编排体在最后一步失败。
/// 修复前（逐语句 autocommit）设置行已先行落库——「设置行残留」即红灯。
#[test]
fn set_base_currency_op_failure_rolls_back_setting() {
    let conn = setup();
    conn.execute(
        "CREATE TRIGGER block_sync_ops BEFORE INSERT ON sync_ops \
         BEGIN SELECT RAISE(ABORT, '测试注入：op 写失败'); END",
        [],
    )
    .unwrap();

    let err = super::base_currency::set_base_currency(&conn, "USD").unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("测试注入：op 写失败"),
        "错误应来自 op 落库失败注入，实际 {err:?}"
    );

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
