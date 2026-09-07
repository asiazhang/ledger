//! Writer 接缝测试共享脚手架：仅限本测试目录（`writer::tests`）内部使用
//! （跨测试模块合并不在此列，见 #250）。
//!
//! 建库与账户夹具已上收统一测试工厂 `crate::test_support`（spec #728 / ADR-0084，
//! 域迁移票 #757）：按检查点结论（#754，中大型域）保留一行转发，调用点不动；
//! 种子簿记戳引用工厂 [`FIXED_NOW`]，零字面量。分类种子与退款来源行为域特有，
//! 按准入规则留薄皮。

use rusqlite::{Connection, params};

use crate::test_support::FIXED_NOW;
use crate::transaction::amount::TransactionKind;
use crate::transaction::writer::{Input, insert_row, normalize};

pub(super) fn setup_db() -> Connection {
    crate::test_support::open()
}

/// 共享锁形态的已初始化内存库：写入口置脏语义测试（`DbState::write` 提交点单点，
/// ADR-0032）需要 DbState 壳，连接本身仍经工厂打开，不绕开统一建库入口。
pub(super) fn setup_db_state() -> crate::db::DbState {
    crate::db::DbState {
        conn: std::sync::Arc::new(std::sync::Mutex::new(crate::test_support::open())),
    }
}

pub(super) fn insert_account(conn: &Connection, id: &str, currency: &str) {
    // 脚手架账户（id 兼名、cash）：工厂账户种子（归一签名，spec #728 / ADR-0084 决策 4）。
    crate::test_support::seed_account(conn, id, id, "cash", currency, 0);
}

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
