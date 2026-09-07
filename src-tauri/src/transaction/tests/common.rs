//! 交易命令域测试共享脚手架：仅限本测试目录（`transactions::tests`）内部使用。
//! 通用夹具（建库两行序、账户种子、投资铺垫）已上收统一测试工厂
//! `crate::test_support`（spec #728 / issue #757 / ADR-0084 决策 4/7），本文件剩余
//! 函数全部为域语义输入构造器——非 DB 夹具，按准入规则（ADR-0084 决策 1）留域内。

use crate::transaction::TransactionInput;
use crate::transaction::amount::TransactionKind;

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

/// 买入输入构造器（投资语义，非 DB 夹具）：本目录唯一真源，batch_create 等直接引用。
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
