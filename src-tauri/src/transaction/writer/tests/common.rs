//! Writer 接缝测试共享脚手架：仅限本测试目录（`writer::tests`）内部使用。
//! 通用夹具（建库两行序、账户种子）已上收统一测试工厂 `crate::test_support`
//! （spec #728 / issue #757 / ADR-0084 决策 4/7）；分类种子为单域夹具按准入规则
//! 留薄皮（簿记戳经 `test_support::FIXED_NOW` 发放），其余为域语义输入构造器
//! 与 Writer 编排铺垫（ADR-0084 决策 1）。

use rusqlite::{Connection, params};

use crate::test_support::FIXED_NOW;
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer::{Input, insert_row, normalize};

pub(super) fn insert_category(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO categories (id,name,kind,created_at,updated_at,version,device_id) \
         VALUES (?1,?1,'expense',?2,?2,1,'test')",
        params![id, FIXED_NOW],
    )
    .unwrap();
}

/// 通用入参构造器。
pub(super) fn input(kind: TransactionKind, amount_cents: i64, account_id: &str) -> Input {
    Input {
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        existing_merchant_id: None,
        policy_id: None,
        existing_policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-01".into(),
    }
}

/// 通过 writer 自身落一笔 expense（normalize + insert_row），作为退款来源。
pub(super) fn insert_source_expense(
    conn: &Connection,
    account_id: &str,
    category_id: Option<&str>,
) -> String {
    let norm = normalize(
        conn,
        &Input {
            kind: TransactionKind::Expense,
            amount_cents: 1000,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            category_id: category_id.map(String::from),
            ..input(TransactionKind::Expense, 1000, account_id)
        },
    )
    .unwrap();
    insert_row(conn, &norm).unwrap()
}
