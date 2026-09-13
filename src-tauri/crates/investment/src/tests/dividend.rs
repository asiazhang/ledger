//! 现金分红（dividend）写入测试（issue #1078 / ADR-0109）。
//!
//! 以行为层公开入口断言外部行为（先例：`split` / `convert` 的测试纪律）：
//! - 分红落一条 `kind=dividend` 交易行（现金腿记 `amount_cents`）+ 一条
//!   `security_transactions` 扩展行（action='dividend'，quantity / price 恒 NULL）；
//! - 到账账户余额按 `account_flow` 既有矩阵加现金（分红计收入，不进支出报表）；
//! - **不要求有持仓**（除息 / 到账错位合法）、不触碰 FIFO 批次、不产生已实现盈亏；
//! - 守卫齐全（码化中文错误）：标的必填且存在、金额 > 0、到账账户在用且币种一致、
//!   无份额 / 单价 / 手续费，携带转入标的 / 转入账户 / 出资账户 / 商户 / 分类 /
//!   保单一律拒绝；
//! - 进 / 出 dividend 的 kind 变更拒绝；就地修改与删除摘除并重建 / 清空扩展行。

use ledger_transaction::amount::TransactionKind;
use ledger_transaction::{
    TransactionInput, create_transaction_internal, delete_transaction_internal,
    update_transaction_internal,
};

use super::super::*;
use super::common::*;
use tauri_app_lib::test_support::{open, seed_account, seed_instrument};

/// 账户实时余额（ADR-0067 口径的权威比对基准，同 split 测试的 `balance_snapshot`）。
fn balance_of(conn: &rusqlite::Connection, account_id: &str) -> i64 {
    ledger_accounts::balance::compute_balance(conn, account_id).unwrap()
}

/// 该交易的证券扩展行投影：`(instrument_id, action, quantity, price_cents, fee_cents)`。
type SecurityRow = (String, String, Option<f64>, Option<i64>, i64);

/// 该交易的证券扩展行，无扩展行返回 None。
fn security_row(conn: &rusqlite::Connection, transaction_id: &str) -> Option<SecurityRow> {
    conn.query_row(
        "SELECT instrument_id, action, quantity, price_cents, fee_cents \
         FROM security_transactions WHERE transaction_id=?1",
        rusqlite::params![transaction_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )
    .ok()
}

fn seed_dividend_scene(conn: &rusqlite::Connection) {
    seed_account(conn, "acc-dv", "股票户", "investment", "CNY", 0);
    seed_account(conn, "acc-dv-cash", "银行卡", "bank", "CNY", 0);
    seed_instrument(conn, "inst-dv", "502010", "证券基金", "CNY", "unknown");
}

/// 普通收入输入（kind 变更守卫的对照组）。
fn make_income_input(account_id: &str, amount_cents: i64, currency: &str) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Income,
        amount_cents,
        currency_code: currency.into(),
        account_id: account_id.into(),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-02-11".into(),
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

#[test]
fn dividend_create_records_cash_leg_and_instrument_extension() {
    let conn = open();
    seed_dividend_scene(&conn);

    // 分红不要求有持仓：零持仓直接录入。
    let write =
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"))
            .unwrap();

    let row: (String, i64, String, i64) = conn
        .query_row(
            "SELECT kind, amount_cents, currency_code, amount_native_cents \
             FROM transactions WHERE id=?1",
            rusqlite::params![write.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(row, ("dividend".to_string(), 3000, "CNY".to_string(), 3000));

    // 扩展行归属标的、无份额 / 单价 / 手续费。
    assert_eq!(
        security_row(&conn, &write.id),
        Some(("inst-dv".to_string(), "dividend".to_string(), None, None, 0))
    );
    // 现金腿按 account_flow=+ 入账（分红计收入）。
    assert_eq!(balance_of(&conn, "acc-dv"), 3000);
    // 不产生持仓、不产生已实现盈亏。
    assert!(list_holdings(&conn).unwrap().is_empty());
    assert_eq!(
        query_realized_pnl_summary(
            &conn,
            &PnlFilter {
                account_id: None,
                instrument_id: None,
            }
        )
        .unwrap()
        .total
        .iter()
        .map(|g| g.realized_pnl_cents)
        .sum::<i64>(),
        0
    );
    // 来源列反查命中标的（读时推导，零新增指针）。
    let sources =
        source_display_by_transaction_ids(&conn, std::slice::from_ref(&write.id)).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].instrument_id, "inst-dv");
}

#[test]
fn dividend_can_land_in_any_active_account() {
    // 到账账户不限于投资账户：分红可直达银行卡 / 现金账户。
    let conn = open();
    seed_dividend_scene(&conn);

    create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-cash", "inst-dv", 1500, "CNY"),
    )
    .unwrap();
    assert_eq!(balance_of(&conn, "acc-dv-cash"), 1500);
}

#[test]
fn dividend_create_guards_return_coded_errors() {
    let conn = open();
    seed_dividend_scene(&conn);
    seed_account(&conn, "acc-dv-usd", "美股账户", "investment", "USD", 0);

    let err = |r: ledger_infra::error::Result<ledger_transaction::TransactionWrite>| {
        r.expect_err("应被拒绝")
    };

    // 标的必填。
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.instrument_id = None;
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-instrument-required");

    // 标的不存在。
    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv", "inst-missing", 3000, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-instrument-not-found");

    // 金额必须 > 0。
    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv", "inst-dv", 0, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-amount-positive");
    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv", "inst-dv", -100, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-amount-positive");

    // 到账账户必须存在且未软删。
    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-missing", "inst-dv", 3000, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-account-not-found");

    // 币种须与账户币种一致。
    let e = err(create_transaction_internal(
        &conn,
        make_dividend_input("acc-dv-usd", "inst-dv", 3000, "CNY"),
    ));
    assert_eq!(e.code().unwrap(), "trade.dividend-currency-mismatch");
    assert!(e.to_string().contains("币种"), "文案应对准币种: {e}");

    // 无份额 / 单价 / 手续费（意图漂移 fail fast）。
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.quantity = Some(1.0);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-quantity-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.price_cents = Some(100);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-price-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.fee_cents = Some(1);
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-fee-forbidden");

    // 转入标的 / 转入账户 / 出资账户。
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.to_instrument_id = Some("inst-other".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-to-instrument-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.to_account_id = Some("acc-dv-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "trade.dividend-to-account-forbidden");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.funding_account_id = Some("acc-dv-cash".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.funding-unsupported");

    // 商户 / 分类 / 保单（行为层参考数据携带准入，dividend 不在任何准入集）。
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.merchant_id = Some("m-1".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.merchant-unsupported");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.category_id = Some("c-1".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.category-unsupported");
    let mut input = make_dividend_input("acc-dv", "inst-dv", 3000, "CNY");
    input.policy_id = Some("p-1".into());
    let e = err(create_transaction_internal(&conn, input));
    assert_eq!(e.code().unwrap(), "transaction.policy-unsupported");
}

#[test]
fn dividend_update_replaces_fields_and_delete_removes_extension() {
    let conn = open();
    seed_dividend_scene(&conn);
    let id =
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"))
            .unwrap()
            .id;

    // 就地修改（全字段替换）：扩展行摘除后重建，金额与账户余额随之更新。
    update_transaction_internal(
        &conn,
        &id,
        make_dividend_input("acc-dv", "inst-dv", 5000, "CNY"),
    )
    .unwrap();
    let amount: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount, 5000);
    assert_eq!(balance_of(&conn, "acc-dv"), 5000);
    assert_eq!(
        security_row(&conn, &id),
        Some(("inst-dv".to_string(), "dividend".to_string(), None, None, 0))
    );

    // 软删：扩展行摘除（交易行软删、余额回退到 0）。
    delete_transaction_internal(&conn, &id).unwrap();
    assert_eq!(security_row(&conn, &id), None);
    assert_eq!(balance_of(&conn, "acc-dv"), 0);
}

#[test]
fn dividend_kind_change_is_forbidden_in_both_directions() {
    let conn = open();
    seed_dividend_scene(&conn);
    let dividend_id =
        create_transaction_internal(&conn, make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"))
            .unwrap()
            .id;
    let income_id = create_transaction_internal(&conn, make_income_input("acc-dv", 1000, "CNY"))
        .unwrap()
        .id;

    // 改出 dividend。
    let e = update_transaction_internal(
        &conn,
        &dividend_id,
        make_income_input("acc-dv", 1000, "CNY"),
    )
    .expect_err("改出 dividend 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.dividend-kind-change-forbidden");

    // 改入 dividend。
    let e = update_transaction_internal(
        &conn,
        &income_id,
        make_dividend_input("acc-dv", "inst-dv", 3000, "CNY"),
    )
    .expect_err("改入 dividend 应被拒绝");
    assert_eq!(e.code().unwrap(), "trade.dividend-kind-change-forbidden");
}
