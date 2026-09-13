//! 出资账户准入校验测试（issue #935 / ADR-0096）：kind 准入、账户类型闭集、
//! 币种一致、存在与未软删，逐条码化错误对码断言。

use rusqlite::Connection;

use tauri_app_lib::test_support;
use tauri_app_lib::transaction::amount::TransactionKind;
use tauri_app_lib::transaction::write::funding::validate_funding_account;

fn setup(conn: &Connection) {
    test_support::seed_account(conn, "acc-cash", "现金", "cash", "CNY", 0);
    test_support::seed_account(conn, "acc-bank", "银行", "bank", "CNY", 0);
    test_support::seed_account(conn, "acc-inv", "证券", "investment", "CNY", 0);
    test_support::seed_account(conn, "acc-debt", "负债", "debt", "CNY", 0);
    test_support::seed_account(conn, "acc-usd", "美元现金", "bank", "USD", 0);
}

#[test]
fn none_funding_always_passes() {
    let conn = test_support::open();
    setup(&conn);
    // 缺省出资账户：任何 kind 恒通过（可选字段，None 即回落既有归因）。
    for kind in [
        TransactionKind::Income,
        TransactionKind::Expense,
        TransactionKind::Transfer,
        TransactionKind::Refund,
        TransactionKind::Buy,
        TransactionKind::Sell,
    ] {
        assert!(validate_funding_account(&conn, kind, None, "CNY").is_ok());
    }
}

#[test]
fn non_buy_sell_kinds_carrying_funding_rejected() {
    let conn = test_support::open();
    setup(&conn);
    for kind in [
        TransactionKind::Income,
        TransactionKind::Expense,
        TransactionKind::Transfer,
        TransactionKind::Refund,
        TransactionKind::Dividend,
        TransactionKind::Split,
    ] {
        let err = validate_funding_account(&conn, kind, Some("acc-cash"), "CNY").unwrap_err();
        assert_eq!(err.code().unwrap(), "transaction.funding-unsupported");
        assert!(err.to_string().contains("不能携带出资账户"), "实际: {err}");
    }
}

#[test]
fn buy_sell_with_cash_like_funding_passes() {
    let conn = test_support::open();
    setup(&conn);
    // 现金类闭集：cash / bank / credit / ewallet / other 逐类型放行（bank 已种）。
    test_support::seed_account(&conn, "acc-credit", "信用卡", "credit", "CNY", 0);
    test_support::seed_account(&conn, "acc-ewallet", "钱包", "ewallet", "CNY", 0);
    test_support::seed_account(&conn, "acc-other", "其他", "other", "CNY", 0);
    for funding in [
        "acc-cash",
        "acc-bank",
        "acc-credit",
        "acc-ewallet",
        "acc-other",
    ] {
        for kind in [TransactionKind::Buy, TransactionKind::Sell] {
            assert!(
                validate_funding_account(&conn, kind, Some(funding), "CNY").is_ok(),
                "{kind} 携带现金类出资账户 {funding} 应放行"
            );
        }
    }
}

#[test]
fn investment_and_receivable_debt_funding_rejected() {
    let conn = &test_support::open();
    setup(conn);
    test_support::seed_account(conn, "acc-receivable", "应收", "receivable", "CNY", 0);
    for funding in ["acc-inv", "acc-debt", "acc-receivable"] {
        let err =
            validate_funding_account(conn, TransactionKind::Buy, Some(funding), "CNY").unwrap_err();
        assert_eq!(err.code().unwrap(), "funding.account-type-unsupported");
    }
}

#[test]
fn missing_or_deleted_funding_account_not_found() {
    let conn = &test_support::open();
    setup(conn);
    // 不存在。
    let err =
        validate_funding_account(conn, TransactionKind::Buy, Some("acc-nope"), "CNY").unwrap_err();
    assert_eq!(err.code().unwrap(), "funding.account-not-found");
    // 已软删除：同码（软删账户不可被新选择）。
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-cash'", [])
        .unwrap();
    let err =
        validate_funding_account(conn, TransactionKind::Buy, Some("acc-cash"), "CNY").unwrap_err();
    assert_eq!(err.code().unwrap(), "funding.account-not-found");
}

#[test]
fn funding_currency_must_match_transaction_currency() {
    let conn = &test_support::open();
    setup(conn);
    let err =
        validate_funding_account(conn, TransactionKind::Buy, Some("acc-usd"), "CNY").unwrap_err();
    assert_eq!(err.code().unwrap(), "funding.currency-mismatch");
    // 同币种放行。
    assert!(validate_funding_account(conn, TransactionKind::Buy, Some("acc-usd"), "USD").is_ok());
}
