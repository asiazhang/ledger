//! 批量写入测试共享脚手架：仅限本测试目录内各子模块使用（跨测试模块合并不在此列，见 #250）。
//!
//! 建库与账户夹具已上收统一测试工厂 `crate::test_support`（spec #728 / ADR-0084，
//! 域迁移票 #757）：按检查点结论（#754，中大型域）保留一行转发，调用点不动。

use rusqlite::Connection;

use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;

pub(super) fn setup() -> Connection {
    crate::test_support::open()
}

pub(super) fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    // 脚手架账户：工厂账户种子（归一签名，spec #728 / ADR-0084 决策 4）。
    crate::test_support::seed_account(conn, id, name, kind, currency, 0);
}

pub(super) fn make_input(
    account_id: &str,
    kind: TransactionKind,
    amount: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind,
        amount_cents: amount,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        idempotency_key: None,
    }
}
