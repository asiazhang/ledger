//! 审计字段统一生成与 native 本位币折算（issue #60：统一经 Writer 落库）。

use super::super::*;
use super::common::{insert_account, make_buy_input, make_input, setup, setup_investment_account};

use crate::transaction::amount::TransactionKind;
use rusqlite::params;

// ---------------------------------------------------------------------------
// issue #60：创建/修改/买入卖出行统一经 Writer 落库
// ---------------------------------------------------------------------------
/// 全部创建路径（通用 kind + buy/sell）落库行带 Writer 统一生成的审计字段：
/// version=1 / is_deleted=0 / created_at==updated_at / device_id 一致——证明
/// create 路径不再散落手写 INSERT（issue #60 验收：审计字段统一生成）。
#[test]
fn create_transaction_internal_audit_fields_uniform_across_kinds() {
    let conn = setup();
    insert_account(&conn, "acc-w", "现金", "cash", "CNY");
    insert_account(&conn, "acc-w2", "银行", "bank", "CNY");
    setup_investment_account(&conn, "acc-inv-w", "inst-w");

    let expense_id = create_transaction_internal(
        &conn,
        make_input("acc-w", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();
    let income_id = create_transaction_internal(
        &conn,
        make_input("acc-w", TransactionKind::Income, 900, "2026-01-02"),
    )
    .unwrap();
    let mut transfer = make_input("acc-w", TransactionKind::Transfer, 300, "2026-01-03");
    transfer.to_account_id = Some("acc-w2".into());
    let transfer_id = create_transaction_internal(&conn, transfer).unwrap();
    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            kind: TransactionKind::Refund,
            amount_cents: 200,
            merchant_id: None,
            refund_of_transaction_id: Some(expense_id.clone()),
            ..make_input("acc-w", TransactionKind::Refund, 100, "2026-01-04")
        },
    )
    .unwrap();
    let buy_id =
        create_transaction_internal(&conn, make_buy_input("acc-inv-w", "inst-w", 2.0, 1000, 0))
            .unwrap();
    let mut sell = make_buy_input("acc-inv-w", "inst-w", 1.0, 1100, 0);
    sell.kind = TransactionKind::Sell;
    sell.date = "2026-01-11".into();
    let sell_id = create_transaction_internal(&conn, sell).unwrap();

    for id in [
        expense_id,
        income_id,
        transfer_id,
        refund_id,
        buy_id,
        sell_id,
    ] {
        let (created_at, updated_at, version, device_id, is_deleted): (
            String,
            String,
            i64,
            String,
            i64,
        ) = conn
            .query_row(
                "SELECT created_at,updated_at,version,device_id,is_deleted \
                 FROM transactions WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            created_at, updated_at,
            "新建行 created_at 与 updated_at 应一致"
        );
        assert_eq!(version, 1, "新建行 version 应为 1");
        assert_eq!(
            device_id,
            crate::db::device_id(),
            "device_id 由 Writer 统一生成"
        );
        assert_eq!(is_deleted, 0, "新建行不应被删除");
    }
}

/// 修改路径经 writer::update_row：保留 created_at、version 递增、updated_at 刷新
/// （issue #60 验收：update 不再走命令层手写 UPDATE）。
#[test]
fn update_transaction_internal_preserves_created_at_and_refreshes_audit() {
    let conn = setup();
    insert_account(&conn, "acc-upd", "现金", "cash", "CNY");
    let id = create_transaction_internal(
        &conn,
        make_input("acc-upd", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap();
    let created_at: String = conn
        .query_row(
            "SELECT created_at FROM transactions WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();

    let mut edited = make_input("acc-upd", TransactionKind::Expense, 900, "2026-01-05");
    edited.note = Some("改后备注".into());
    update_transaction_internal(&conn, &id, edited).unwrap();

    let (created_at_after, updated_at, version): (String, String, i64) = conn
        .query_row(
            "SELECT created_at,updated_at,version FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(created_at_after, created_at, "修改应保留 created_at");
    assert!(!updated_at.is_empty(), "修改应刷新 updated_at");
    assert_eq!(version, 2, "修改后 version 应递增");
}

/// 通用 kind 的本位币折算改经 Amount 接缝（基准为全局默认币种，issue #60 / spec #52）：
/// USD 账户 + USD 金额按汇率折算到 CNY，而非按账户币种 1:1 落库。
#[test]
fn create_transaction_internal_generic_converts_native_via_amount_seam() {
    let conn = setup();
    insert_account(&conn, "acc-usd", "美元", "cash", "USD");
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-w','USD','CNY',7.2,'2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            currency_code: "USD".into(),
            ..make_input("acc-usd", TransactionKind::Expense, 10000, "2026-01-01")
        },
    )
    .unwrap();
    let (amount_native_cents, currency_code): (i64, String) = conn
        .query_row(
            "SELECT amount_native_cents, currency_code FROM transactions WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(currency_code, "USD", "原始币种保留");
    assert_eq!(
        amount_native_cents, 72000,
        "本位币金额应经 Amount 接缝折算到全局默认币种"
    );
}
