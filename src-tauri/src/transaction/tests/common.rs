//! 交易命令域测试共享脚手架：仅限本测试目录（`transactions::tests`）内部使用。
//!
//! 建库与跨域重复夹具（账户、投资铺垫）已上收统一测试工厂 `crate::test_support`
//! （spec #728 / ADR-0084，域迁移票 #757）：本薄皮按检查点结论（#754，中大型域）
//! 保留一行转发，调用点不动、种子常数单点化。域语义构造器（`make_input` 等
//! TransactionInput 构造，非 DB 夹具）按准入规则留域内。

use crate::transaction::TransactionInput;
use rusqlite::Connection;

use crate::transaction::amount::TransactionKind;

pub(crate) fn setup() -> Connection {
    crate::test_support::open()
}

pub(crate) fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    // 脚手架账户：工厂账户种子（归一签名，spec #728 / ADR-0084 决策 4）。
    crate::test_support::seed_account(conn, id, name, kind, currency, 0);
}

pub(crate) fn make_input(
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

pub(crate) fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        idempotency_key: None,
    }
}
