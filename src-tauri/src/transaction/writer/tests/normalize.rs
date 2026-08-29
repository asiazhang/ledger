//! normalize 归一化校验：通用 kind 直通、金额 > 0、transfer 必填目标账户、
//! 仅接受通用 kind（buy/sell/dividend/split 拒绝）、本位币折算（Amount 接缝）。

use rusqlite::{Connection, params};

use crate::transaction::amount::TransactionKind;
use crate::transaction::writer::{Input, normalize};

use super::common::{input, insert_account, insert_category, setup_db};

fn insert_rate(conn: &Connection, base: &str, quote: &str, rate: f64) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-1',?1,?2,?3,'2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        params![base, quote, rate],
    )
    .unwrap();
}

// ---------------------------------------------------------------------------
// normalize：通用 kind 直通
// ---------------------------------------------------------------------------

/// income 直通：字段原样保留，本位币与原始币种 1:1（CNY）。
#[test]
fn normalize_income_passthrough() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    for bad in [0, -1, -500] {
        let err = normalize(&conn, &input(TransactionKind::Expense, bad, "acc")).unwrap_err();
        assert_eq!(err.to_string(), "参数错误: 金额必须大于 0", "金额 {bad}");
    }
}

// ---------------------------------------------------------------------------
// normalize：transfer 必填目标账户
// ---------------------------------------------------------------------------

/// transfer 缺 `to_account_id` 报错（文案与命令层既有断言一致）。
#[test]
fn normalize_transfer_requires_to_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    let err = normalize(&conn, &input(TransactionKind::Transfer, 3000, "acc-a")).unwrap_err();
    assert_eq!(err.to_string(), "参数错误: 转账必须指定目标账户");
}

/// transfer 带 `to_account_id` 时归一化成功，目标账户透传。
#[test]
fn normalize_transfer_passes_to_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "CNY");
    insert_account(&conn, "acc-b", "CNY");
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
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
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

/// 非默认币种按 Amount 接缝折算到全局默认币种（CNY），与账户币种无关。
#[test]
fn normalize_converts_via_amount_seam_to_default_currency() {
    let conn = setup_db();
    insert_account(&conn, "acc-usd", "USD");
    insert_rate(&conn, "USD", "CNY", 7.2);
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

/// 非默认币种且无汇率 → 报错，不静默 1:1 混币种。
#[test]
fn normalize_errors_without_rate_for_non_default_currency() {
    let conn = setup_db();
    insert_account(&conn, "acc-jpy", "JPY");
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
