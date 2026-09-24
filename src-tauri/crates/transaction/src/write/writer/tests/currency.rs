//! 币种一致性守卫（ADR-0134 / #1770）：通用 kind 的交易币种必须等于所涉账户
//! 币种（余额按账户币种累计的前提，#1769）——income / expense 单端、transfer
//! 两端各比一次；守卫只判「读得到的币种一致」，账户行读不到时不判定（存活
//! 校验不属本守卫：本地形态刻意无存活校验 ADR-0105 决策 4，重放臂存活校验
//! 先行）；refund 继承原支出、守卫照跑（防御性）。

use ledger_infra::error::AppError;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::writer::{
    Input, NormalizedRow, insert_row, normalize,
};

use super::common::input;
use tauri_app_lib::test_support;

/// 期望币种参数断言（params 契约：[账户币种, 交易币种]，供 zh / en 模板插值）。
fn assert_currency_mismatch(err: AppError, expected: [&str; 2]) {
    match err {
        AppError::Coded { code, params, .. } => {
            assert_eq!(code, "transaction.currency-mismatch");
            assert_eq!(
                params,
                expected.map(String::from).to_vec(),
                "params 契约 = [账户币种, 交易币种]"
            );
        }
        other => panic!("应为 transaction.currency-mismatch 码化错误，实际: {other:?}"),
    }
}

/// income / expense：账户币种（USD）≠ 交易币种（CNY）→ 码化拒绝。
#[test]
fn normalize_rejects_currency_mismatching_single_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    for kind in [TransactionKind::Income, TransactionKind::Expense] {
        let err = normalize(
            &conn,
            &Input {
                currency_code: "CNY".into(),
                ..input(kind, 1000, "acc-usd")
            },
        )
        .unwrap_err();
        assert_currency_mismatch(err, ["USD", "CNY"]);
    }
}

/// 非默认币种交易与账户币种一致时放行：守卫比的是账户币种，不是全局默认币种
/// （USD 账户记 USD 账是常态，折算照走 Amount 接缝）。
#[test]
fn normalize_allows_matching_non_default_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-w", "USD", "CNY", "2025-12-29", 7.2);
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(norm.currency_code, "USD");
    assert_eq!(norm.amount_native_cents, 72000);
}

/// transfer：转出端一致、转入端不一致 → 码化拒绝（两端各比一次）。
#[test]
fn normalize_rejects_transfer_to_end_currency_mismatch() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    let err = normalize(
        &conn,
        &Input {
            currency_code: "CNY".into(),
            to_account_id: Some("acc-usd".into()),
            ..input(TransactionKind::Transfer, 3000, "acc-cny")
        },
    )
    .unwrap_err();
    assert_currency_mismatch(err, ["USD", "CNY"]);
}

/// transfer：两端币种均与交易币种一致 → 放行（同币种转账是常态）。
#[test]
fn normalize_allows_transfer_with_both_ends_matching() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd-a", "acc-usd-a", "cash", "USD", 0);
    test_support::seed_account(&conn, "acc-usd-b", "acc-usd-b", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-w", "USD", "CNY", "2025-12-29", 7.2);
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            to_account_id: Some("acc-usd-b".into()),
            ..input(TransactionKind::Transfer, 3000, "acc-usd-a")
        },
    )
    .unwrap();
    assert_eq!(norm.to_account_id.as_deref(), Some("acc-usd-b"));
}

/// 账户行读不到时不判定：本地形态刻意无账户存活校验（ADR-0105 决策 4），
/// 本守卫不得夹带存在性拒绝——只判「读得到的币种一致」。
#[test]
fn normalize_skips_guard_when_account_row_unreadable() {
    let conn = test_support::open();
    let norm = normalize(&conn, &input(TransactionKind::Income, 1000, "no-such-acc")).unwrap();
    assert_eq!(norm.account_id, "no-such-acc");
}

/// refund 防御臂：来源支出的币种与账户不一致（守卫上线前的存量脏行，经
/// insert_row 直落绕过 normalize 构造），退款继承后守卫照跑拒绝。
#[test]
fn normalize_refund_guard_runs_on_inherited_dirty_source() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-cny", "acc-cny", "cash", "CNY", 0);
    let dirty = NormalizedRow {
        kind: TransactionKind::Expense,
        amount_cents: 1000,
        currency_code: "USD".into(),
        amount_native_cents: 1000,
        fx_rate_used: None,
        fx_rate_source: None,
        account_id: "acc-cny".into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-01".into(),
    };
    let source_id = insert_row(&conn, &dirty).unwrap();

    let err = normalize(
        &conn,
        &Input {
            kind: TransactionKind::Refund,
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc-cny")
        },
    )
    .unwrap_err();
    // 继承值：账户 acc-cny（CNY）、币种 USD。
    assert_currency_mismatch(err, ["CNY", "USD"]);
}
