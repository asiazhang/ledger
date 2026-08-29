//! 商户（merchant_id）写入语义：存在商户透传、不存在/软删商户拒绝、退款
//! 继承原支出商户、修改路径 unchanged 引用跳过在用校验。

use rusqlite::{Connection, params};

use crate::transaction::amount::TransactionKind;
use crate::transaction::writer::{Input, insert_row, normalize};

use super::common::{input, insert_account, insert_source_expense, setup_db};

// ---------------------------------------------------------------------------
// normalize：商户（merchant_id）
// ---------------------------------------------------------------------------

fn insert_merchant(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name],
    )
    .unwrap();
}

/// income/expense 携带存在的商户 → 归一化行透传 merchant_id。
#[test]
fn normalize_merchant_passthrough() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-jd".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-jd"));
}

/// 携带不存在的商户 → 明确错误（商户不存在）。
#[test]
fn normalize_merchant_not_found_is_rejected() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("no-such-merchant".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "参数错误: 商户不存在或已删除: no-such-merchant"
    );
}

/// 携带已软删除的商户 → 明确错误（软删商户不可再被新交易选择）。
#[test]
fn normalize_soft_deleted_merchant_is_rejected_for_new_txn() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            ..input(TransactionKind::Income, 1000, "acc")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
}

/// 退款继承原支出的商户（与账户/币种/分类同款继承语义）：忽略调用方填的 merchant_id，
/// 取原支出商户。
#[test]
fn normalize_refund_inherits_source_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc-src", "CNY");
    insert_merchant(&conn, "mer-jd", "京东");
    insert_merchant(&conn, "mer-pdd", "拼多多");
    // 落一笔带商户的原支出
    let source_norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-jd".into()),
            ..input(TransactionKind::Expense, 1000, "acc-src")
        },
    )
    .unwrap();
    let source_id = insert_row(&conn, &source_norm).unwrap();

    // 退款调用方填了另一个商户 → 仍继承原支出的京东
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-pdd".into()),
            refund_of_transaction_id: Some(source_id.clone()),
            ..input(TransactionKind::Refund, 200, "acc-src")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-jd"));
    assert_eq!(
        norm.refund_of_transaction_id.as_deref(),
        Some(source_id.as_str())
    );
}

/// 原支出无商户 → 退款商户为空。
#[test]
fn normalize_refund_without_source_merchant_has_none() {
    let conn = setup_db();
    insert_account(&conn, "acc-src", "CNY");
    let source_id = insert_source_expense(&conn, "acc-src", None);
    let norm = normalize(
        &conn,
        &Input {
            refund_of_transaction_id: Some(source_id),
            ..input(TransactionKind::Refund, 200, "acc-src")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id, None);
}

/// 修改路径保持历史引用：提交商户与该行当前商户相同（`existing_merchant_id`）时
/// 跳过在用校验——软删商户的历史交易仍可修改其他字段（与账户/分类更新语义一致）。
#[test]
fn normalize_keeps_unchanged_merchant_even_if_soft_deleted() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    // 提交值与既有值相同：跳过在用校验，归一化成功。
    let norm = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            existing_merchant_id: Some("mer-dead".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap();
    assert_eq!(norm.merchant_id.as_deref(), Some("mer-dead"));
}

/// 修改路径改选其他商户仍按新选择校验在用：目标为软删商户 → 拒绝。
#[test]
fn normalize_rejects_changing_to_soft_deleted_merchant() {
    let conn = setup_db();
    insert_account(&conn, "acc", "CNY");
    insert_merchant(&conn, "mer-old", "旧商户");
    insert_merchant(&conn, "mer-dead", "已删商户");
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-dead'", [])
        .unwrap();
    let err = normalize(
        &conn,
        &Input {
            merchant_id: Some("mer-dead".into()),
            existing_merchant_id: Some("mer-old".into()),
            ..input(TransactionKind::Expense, 1500, "acc")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
}
