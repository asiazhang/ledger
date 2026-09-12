//! 批量写入测试共享脚手架：仅限本测试目录内各子模块使用。通用夹具（建库两行序、
//! 账户种子）已上收统一测试工厂 `tauri_app_lib::test_support`（spec #728 / issue #757 /
//! ADR-0084 决策 4/7），本文件剩余函数为域语义输入构造器——非 DB 夹具，
//! 按准入规则（ADR-0084 决策 1）留域内。

use tauri_app_lib::transaction::TransactionInput;
use tauri_app_lib::transaction::amount::TransactionKind;

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
        funding_account_id: None,
        note: None,
        date: date.into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
    }
}
