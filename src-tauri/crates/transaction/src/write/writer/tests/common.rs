//! Writer 接缝测试共享脚手架：仅限本测试目录（`writer::tests`）内部使用。
//! 通用夹具（建库两行序、账户种子）已上收统一测试工厂 `tauri_app_lib::test_support`
//! （spec #728 / issue #757 / ADR-0084 决策 4/7）；分类种子经 crate 根测试目录单点
//! `crate::tests::common::seed_category`（公开写入口，issue #1814），其余为域语义
//! 输入构造器与 Writer 编排铺垫（ADR-0084 决策 1）。

use rusqlite::Connection;

use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::ledger_transaction::write::writer::{Input, insert_row, normalize};

/// 通用入参构造器。
pub(super) fn input(kind: TransactionKind, amount_cents: i64, account_id: &str) -> Input {
    Input {
        kind,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        existing_merchant_id: None,
        policy_id: None,
        existing_policy_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-01".into(),
        fx_rate: None,
        fx_edit_baseline: None,
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
