//! normalize 归一化校验：通用 kind 直通、金额 > 0、transfer 必填目标账户、
//! 仅接受通用 kind（buy/sell/dividend/split 拒绝）、本位币折算（Amount 接缝）。

use ledger_infra::error::AppError;
use tauri_app_lib::ledger_transaction::amount::{FxRateSource, TransactionKind};
use tauri_app_lib::ledger_transaction::write::writer::{Input, normalize};

use super::common::{input, insert_category};
use tauri_app_lib::test_support;

// ---------------------------------------------------------------------------
// normalize：通用 kind 直通
// ---------------------------------------------------------------------------

/// income 直通：字段原样保留，本位币与原始币种 1:1（CNY）。
#[test]
fn normalize_income_passthrough() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &Input {
            note: Some("工资".into()),
            ..input(TransactionKind::Income, 5000, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.kind, TransactionKind::Income);
    assert_eq!(norm.amount_cents, 5000);
    assert_eq!(norm.currency_code, "CNY");
    assert_eq!(norm.amount_native_cents, 5000, "本位币与原始币种应 1:1");
    assert_eq!(norm.account_id, "acc");
    assert_eq!(norm.to_account_id, None);
    assert_eq!(norm.refund_of_transaction_id, None);
    assert_eq!(norm.note.as_deref(), Some("工资"));
    assert_eq!(norm.date, "2026-01-01");
}

/// expense 可选字段（分类/备注）透传。
#[test]
fn normalize_expense_passes_optional_fields() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    insert_category(&conn, "cat-food");
    let norm = normalize(
        &conn,
        &Input {
            category_id: Some("cat-food".into()),
            note: Some("午餐".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.category_id.as_deref(), Some("cat-food"));
    assert_eq!(norm.note.as_deref(), Some("午餐"));
}

// ---------------------------------------------------------------------------
// normalize：金额 > 0 校验
// ---------------------------------------------------------------------------

/// 金额为 0 或负数均应报错。
#[test]
fn normalize_rejects_non_positive_amount() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    for bad in [0, -1, -500] {
        let err = normalize(&conn, &input(TransactionKind::Expense, bad, "acc")).unwrap_err();
        assert_eq!(err.to_string(), "金额必须大于 0", "金额 {bad}");
    }
}

// ---------------------------------------------------------------------------
// normalize：transfer 必填目标账户
// ---------------------------------------------------------------------------

/// transfer 缺 `to_account_id` 报错（文案与命令层既有断言一致）。
#[test]
fn normalize_transfer_requires_to_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    let err = normalize(&conn, &input(TransactionKind::Transfer, 3000, "acc-a")).unwrap_err();
    assert_eq!(err.to_string(), "转账必须指定目标账户");
}

/// transfer 带 `to_account_id` 时归一化成功，目标账户透传。
#[test]
fn normalize_transfer_passes_to_account() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-a", "acc-a", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-b", "acc-b", "cash", "CNY", 0);
    let norm = normalize(
        &conn,
        &Input {
            to_account_id: Some("acc-b".into()),
            ..input(TransactionKind::Transfer, 3000, "acc-a")
        },
    )
    .unwrap();
    assert_eq!(norm.account_id, "acc-a");
    assert_eq!(norm.to_account_id.as_deref(), Some("acc-b"));
    assert_eq!(norm.refund_of_transaction_id, None);
}

// ---------------------------------------------------------------------------
// normalize：仅接受通用 kind
// ---------------------------------------------------------------------------

/// buy/sell/dividend/split 不属于 writer::normalize 职责，应报错防误用。
#[test]
fn normalize_rejects_non_generic_kinds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc", "acc", "cash", "CNY", 0);
    for kind in [
        TransactionKind::Buy,
        TransactionKind::Sell,
        TransactionKind::Dividend,
        TransactionKind::Split,
    ] {
        let err = normalize(&conn, &input(kind, 1000, "acc")).unwrap_err();
        assert!(
            err.to_string().contains("仅处理通用交易类型"),
            "kind={kind:?} 应被拒绝，实际: {err}"
        );
    }
}

// ---------------------------------------------------------------------------
// normalize：本位币折算（Amount 接缝）
// ---------------------------------------------------------------------------

/// 非默认币种按 Amount 接缝按交易日入口折算到全局默认币种（CNY），与账户币种
/// 无关（#1547：取数改汇率历史周点，当期表不再服务写路径——本测试不种当期行，
/// 删除「按交易日取数」接线（改回当期入口）即红）。
#[test]
fn normalize_converts_via_amount_seam_to_default_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    // 交易日期 2026-01-01（周四）所属周的周一为 2025-12-29：只种该周历史点。
    test_support::seed_fx_rate_history(&conn, "fxh-w", "USD", "CNY", "2025-12-29", 7.2);
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(norm.amount_cents, 10000);
    assert_eq!(norm.currency_code, "USD");
    // 基准为全局默认币种（CNY），即使账户是 USD 也不按账户币种 1:1
    assert_eq!(norm.amount_native_cents, 72000);
}

/// 显式汇率优先（#1549）：入参携 `fx_rate` 的行跳过序列查询、按给定值折算，
/// 留痕标为 `explicit`；不带显式汇率的行行为不变（序列点仍生效）。
#[test]
fn normalize_explicit_fx_rate_skips_series_and_traces_explicit() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    test_support::seed_fx_rate_history(&conn, "fxh-w", "USD", "CNY", "2025-12-29", 7.2);

    // 带显式汇率：序列有点仍不用，按给定值折算并标 `explicit`。
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            fx_rate: Some(7.0),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(
        norm.amount_native_cents, 70000,
        "native = amount × 显式汇率"
    );
    assert_eq!(norm.fx_rate_used, Some(7.0));
    assert_eq!(
        norm.fx_rate_source,
        Some(FxRateSource::Explicit),
        "留痕标为显式"
    );

    // 不带显式汇率：行为与 #1547 逐位一致（同周序列点 7.2 生效）。
    let norm = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap();
    assert_eq!(norm.amount_native_cents, 72000);
    assert_eq!(norm.fx_rate_source, Some(FxRateSource::Series));
}

/// 显式汇率非法（非正）→ 码化错误，归一化失败整笔不落库（#1549 验收 3）。
#[test]
fn normalize_explicit_fx_rate_non_positive_coded_error() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-usd", "acc-usd", "cash", "USD", 0);
    let err = normalize(
        &conn,
        &Input {
            currency_code: "USD".into(),
            fx_rate: Some(0.0),
            ..input(TransactionKind::Expense, 10000, "acc-usd")
        },
    )
    .unwrap_err();
    match err {
        AppError::Coded { code, .. } => assert_eq!(code, "fx.explicit-rate-non-positive"),
        other => panic!("应为码化错误，实际: {other}"),
    }
}

/// 非默认币种且无汇率 → 报错，不静默 1:1 混币种。
#[test]
fn normalize_errors_without_rate_for_non_default_currency() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-jpy", "acc-jpy", "cash", "JPY", 0);
    let err = normalize(
        &conn,
        &Input {
            currency_code: "JPY".into(),
            ..input(TransactionKind::Expense, 10000, "acc-jpy")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("汇率"), "实际: {err}");
}
