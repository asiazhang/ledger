//! refund 链归一化：必须关联原支出、继承原支出账户/币种/分类（忽略调用方
//! 填写值），以及来源非支出/不存在/已软删除的拒绝路径。

use rusqlite::params;

use crate::error::{AppError, ErrClass};
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer::{Input, insert_row, normalize};

use super::common::{input, insert_account, insert_category, insert_source_expense, setup_db};

// ---------------------------------------------------------------------------
// normalize：退款继承原支出
// ---------------------------------------------------------------------------

/// refund 未关联原支出交易 → 报错。
#[test]
fn normalize_refund_requires_source_id() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let err = normalize(&conn, &input(TransactionKind::Refund, 200, "acc")).unwrap_err();
    assert_eq!(err.to_string(), "退款必须关联原支出交易");
}

/// 退款继承原支出的账户/币种/分类，忽略调用方填写的 account_id/currency_code/category_id。
#[test]
fn normalize_refund_inherits_source_fields() {
    let conn = setup_db();
    insert_account(&conn, "acc-src", "CNY");
    insert_account(&conn, "acc-other", "USD");
    insert_category(&conn, "cat-src");
    let source_id = insert_source_expense(&conn, "acc-src", Some("cat-src"));

    let norm = normalize(
        &conn,
        &Input {
            kind: TransactionKind::Refund,
            amount_cents: 200,
            currency_code: "USD".into(),
            account_id: "acc-other".into(),
            category_id: Some("cat-other".into()),
            refund_of_transaction_id: Some(source_id.clone()),
            ..input(TransactionKind::Refund, 200, "acc-other")
        },
    )
    .unwrap();
    // 继承原支出：账户/币种/分类均为来源值，而非调用方填的字段
    assert_eq!(norm.account_id, "acc-src");
    assert_eq!(norm.currency_code, "CNY");
    assert_eq!(norm.category_id.as_deref(), Some("cat-src"));
    assert_eq!(
        norm.refund_of_transaction_id.as_deref(),
        Some(source_id.as_str())
    );
    // 金额与日期仍是调用方值
    assert_eq!(norm.amount_cents, 200);
    assert_eq!(norm.date, "2026-01-01");
}

/// 关联的交易不是支出（income）→ 报错。
#[test]
fn normalize_refund_rejects_non_expense_source() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let income_norm = normalize(&conn, &input(TransactionKind::Income, 1000, "acc")).unwrap();
    let income_id = insert_row(&conn, &income_norm).unwrap();

    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(income_id),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "退款只能关联支出交易");
}

/// 关联的原支出不存在 → NotFound。
#[test]
fn normalize_refund_source_not_found() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some("no-such-id".into()),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "应返回 NotFound，实际: {err:?}"
    );
}

/// 关联的原支出已软删除 → 视为不存在（NotFound）。
#[test]
fn normalize_refund_source_soft_deleted_is_not_found() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let source_id = insert_source_expense(&conn, "acc", None);
    conn.execute(
        "UPDATE transactions SET is_deleted=1 WHERE id=?1",
        params![source_id],
    )
    .unwrap();

    let err = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc")
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}
