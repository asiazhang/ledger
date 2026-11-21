use crate::tests::common::make_input;
use rusqlite::Connection;
use tauri_app_lib::ledger_transaction::*;
use tauri_app_lib::ledger_transaction::{TransactionInput, TransactionListFilter};
use tauri_app_lib::test_support;

use ledger_infra::signals::{Signal, WriteEvidence, WriteOp, signals_for};
use rusqlite::params;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;

fn insert_merchant(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?3,1,'test',0)",
        params![id, name, test_support::FIXED_NOW],
    )
    .unwrap();
}

#[test]
fn create_income_expense_transfer_refund_with_merchant() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-m", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-m-to", "银行", "bank", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    let income_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            ..make_input("acc-m", TransactionKind::Income, 500, "2026-01-02")
        },
    )
    .unwrap()
    .id;

    let transfer_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Transfer,
            merchant_id: Some("mer-jd".into()),
            to_account_id: Some("acc-m-to".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-02")
        },
    )
    .unwrap()
    .id;

    let refund_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Refund,
            merchant_id: Some("mer-jd".into()),
            refund_of_transaction_id: Some(expense_id.clone()),
            ..make_input("acc-m", TransactionKind::Refund, 100, "2026-01-03")
        },
    )
    .unwrap()
    .id;

    for (id, expect_merchant) in [
        (&expense_id, Some("mer-jd")),
        (&income_id, Some("mer-jd")),
        (&transfer_id, Some("mer-jd")),
        (&refund_id, Some("mer-jd")),
    ] {
        let merchant_id: Option<String> = conn
            .query_row(
                "SELECT merchant_id FROM transactions WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            merchant_id.as_deref(),
            expect_merchant,
            "交易 {id} 商户不符"
        );
    }
}

#[test]
fn create_txn_with_merchant_rejected_for_non_merchant_kinds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-m", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-m-inv", "证券", "investment", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    for kind in [TransactionKind::Buy, TransactionKind::Sell] {
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                kind,
                merchant_id: Some("mer-jd".into()),
                instrument_id: Some("inst-x".into()),
                quantity: Some(10.0),
                price_cents: Some(1000),
                fee_cents: Some(0),
                ..make_input("acc-m-inv", kind, 10000, "2026-01-01")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带商户"),
            "{kind} 应报「不能携带商户」，实际: {err}"
        );
    }

    for kind in [TransactionKind::Dividend, TransactionKind::Split] {
        let err = create_transaction_internal(
            &conn,
            TransactionInput {
                policy_id: None,
                kind,
                merchant_id: Some("mer-jd".into()),
                ..make_input("acc-m", kind, 60, "2026-01-01")
            },
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("不能携带商户"),
            "{kind} 携带商户应报「不能携带商户」，实际: {err}"
        );
    }

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "拒绝的交易不应落库");
}

#[test]
fn update_expense_to_transfer_with_merchant_succeeds() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-m", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-m-to", "银行", "bank", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let id = create_transaction_internal(
        &conn,
        make_input("acc-m", TransactionKind::Expense, 500, "2026-01-01"),
    )
    .unwrap()
    .id;

    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Transfer,
            merchant_id: Some("mer-jd".into()),
            to_account_id: Some("acc-m-to".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-02")
        },
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.kind, TransactionKind::Transfer);
    assert_eq!(t.merchant_id.as_deref(), Some("mer-jd"));
}

#[test]
fn update_transfer_keeps_existing_merchant() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-m", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-m-to", "银行", "bank", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Transfer,
            merchant_id: Some("mer-jd".into()),
            to_account_id: Some("acc-m-to".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Transfer,
            merchant_id: Some("mer-jd".into()),
            to_account_id: Some("acc-m-to".into()),
            note: Some("改备注".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-01")
        },
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.note.as_deref(), Some("改备注"));
    assert_eq!(t.merchant_id.as_deref(), Some("mer-jd"), "商户引用应保留");
}

#[test]
fn update_transfer_with_new_merchant_name_creates_and_attaches() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-m", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-m-to", "银行", "bank", "CNY", 0);

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            kind: TransactionKind::Transfer,
            to_account_id: Some("acc-m-to".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    let evidence = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_name: Some("魏有鼎".into()),
            policy_id: None,
            kind: TransactionKind::Transfer,
            to_account_id: Some("acc-m-to".into()),
            ..make_input("acc-m", TransactionKind::Transfer, 3000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(evidence, WriteEvidence::MerchantCreated(true));
    assert_eq!(active_merchant_count(&conn), 1);
    assert!(
        merchant_id_of(&conn, &id).is_some(),
        "修改后交易应引用即建的商户"
    );
}

#[test]
fn read_back_carries_merchant_id_after_merchant_soft_delete_and_rename() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-m", "现金", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            note: Some("备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-jd'", [])
        .unwrap();
    conn.execute("UPDATE merchants SET name='京东商城' WHERE id='mer-jd'", [])
        .unwrap();

    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.merchant_id.as_deref(), Some("mer-jd"));

    let listed = list_transactions_internal(&conn, &TransactionListFilter::default()).unwrap();
    assert_eq!(listed.items[0].merchant_id.as_deref(), Some("mer-jd"));

    let searched = tauri_app_lib::ledger_transaction::search_transactions_internal(
        &conn, "备注", 1, 10, None, None, None, None,
    )
    .unwrap();
    assert_eq!(searched.items.len(), 1);
    assert_eq!(searched.items[0].merchant_id.as_deref(), Some("mer-jd"));
}

#[test]
fn update_historical_txn_keeps_soft_deleted_merchant() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-m", "现金", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");
    let id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            note: Some("旧备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;
    conn.execute("UPDATE merchants SET is_deleted=1 WHERE id='mer-jd'", [])
        .unwrap();

    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            note: Some("新备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.note.as_deref(), Some("新备注"));
    assert_eq!(t.merchant_id.as_deref(), Some("mer-jd"), "商户引用应保留");

    insert_merchant(&conn, "mer-pdd", "拼多多");
    update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-pdd".into()),
            note: Some("新备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.merchant_id.as_deref(), Some("mer-pdd"));

    let err = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            note: Some("新备注".into()),
            ..make_input("acc-m", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("商户不存在或已删除"));
    let t = get_transaction_internal(&conn, &id).unwrap();
    assert_eq!(t.merchant_id.as_deref(), Some("mer-pdd"));
}

fn active_merchant_count(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM merchants WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn merchant_id_of(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT merchant_id FROM transactions WHERE id=?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn create_with_new_merchant_name_reports_merchant_created() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);

    let write = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("盒马".into()),
            policy_id: None,
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(
        write.evidence,
        WriteEvidence::MerchantCreated(true),
        "新名字即建应为真"
    );
    assert_eq!(active_merchant_count(&conn), 1, "商户行应随交易落库");
    let merchant_id = merchant_id_of(&conn, &write.id).expect("交易应引用即建的商户");
    let name: String = conn
        .query_row(
            "SELECT name FROM merchants WHERE id=?1",
            params![merchant_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "盒马");

    assert_eq!(
        signals_for(WriteOp::CreateTransaction, write.evidence),
        &[Signal::LedgerChanged]
    );
}

#[test]
fn create_with_hit_merchant_name_reports_reuse_and_zero_signal() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let write = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("京东".into()),
            policy_id: None,
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(write.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(active_merchant_count(&conn), 1, "命中复用不新建");
    assert_eq!(signals_for(WriteOp::CreateTransaction, write.evidence), &[]);
}

#[test]
fn create_without_new_name_reports_false_evidence() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let with_id = create_transaction_internal(
        &conn,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    let without = create_transaction_internal(
        &conn,
        make_input("acc-ev", TransactionKind::Expense, 500, "2026-01-02"),
    )
    .unwrap();

    assert_eq!(with_id.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(without.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, with_id.evidence),
        &[]
    );
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, without.evidence),
        &[]
    );
}

#[test]
fn create_refund_ignores_merchant_name_and_reports_false() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let expense_id = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("京东".into()),
            policy_id: None,
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap()
    .id;

    let refund = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("不存在的商户".into()),
            policy_id: None,
            refund_of_transaction_id: Some(expense_id),
            ..make_input("acc-ev", TransactionKind::Refund, 100, "2026-01-02")
        },
    )
    .unwrap();

    assert_eq!(refund.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(active_merchant_count(&conn), 1, "refund 不即建商户");
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, refund.evidence),
        &[]
    );
}

#[test]
fn create_transfer_with_new_merchant_name_reports_merchant_created() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-ev-to", "银行", "bank", "CNY", 0);

    let write = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("魏有鼎".into()),
            policy_id: None,
            to_account_id: Some("acc-ev-to".into()),
            ..make_input("acc-ev", TransactionKind::Transfer, 3000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(
        write.evidence,
        WriteEvidence::MerchantCreated(true),
        "transfer 带新名字即建应为真"
    );
    assert_eq!(active_merchant_count(&conn), 1);
    assert!(
        merchant_id_of(&conn, &write.id).is_some(),
        "交易应引用即建的商户"
    );
    assert_eq!(
        signals_for(WriteOp::CreateTransaction, write.evidence),
        &[Signal::LedgerChanged]
    );
}

#[test]
fn create_transfer_with_hit_merchant_name_reports_reuse_and_zero_signal() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);
    test_support::seed_account(&conn, "acc-ev-to", "银行", "bank", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let write = create_transaction_internal(
        &conn,
        TransactionInput {
            merchant_name: Some("京东".into()),
            policy_id: None,
            to_account_id: Some("acc-ev-to".into()),
            ..make_input("acc-ev", TransactionKind::Transfer, 3000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(write.evidence, WriteEvidence::MerchantCreated(false));
    assert_eq!(active_merchant_count(&conn), 1, "命中复用不新建");
    assert_eq!(merchant_id_of(&conn, &write.id).as_deref(), Some("mer-jd"));
    assert_eq!(signals_for(WriteOp::CreateTransaction, write.evidence), &[]);
}

#[test]
fn update_to_new_merchant_name_reports_merchant_created() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);
    let id = create_transaction_internal(
        &conn,
        make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;

    let evidence = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_name: Some("物美".into()),
            policy_id: None,
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();

    assert_eq!(evidence, WriteEvidence::MerchantCreated(true));
    assert_eq!(active_merchant_count(&conn), 1);
    assert_eq!(
        signals_for(WriteOp::UpdateTransaction, evidence),
        &[Signal::LedgerChanged]
    );
}

#[test]
fn update_reusing_merchant_reports_false_and_zero_signal() {
    let conn = test_support::open();
    test_support::seed_account(&conn, "acc-ev", "现金", "cash", "CNY", 0);
    insert_merchant(&conn, "mer-jd", "京东");

    let id = create_transaction_internal(
        &conn,
        make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01"),
    )
    .unwrap()
    .id;
    let hit = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            merchant_name: Some("京东".into()),
            policy_id: None,
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    assert_eq!(hit, WriteEvidence::MerchantCreated(false));
    assert_eq!(signals_for(WriteOp::UpdateTransaction, hit), &[]);

    let keep = update_transaction_internal(
        &conn,
        &id,
        TransactionInput {
            policy_id: None,
            merchant_id: Some("mer-jd".into()),
            note: Some("只改备注".into()),
            ..make_input("acc-ev", TransactionKind::Expense, 1000, "2026-01-01")
        },
    )
    .unwrap();
    assert_eq!(keep, WriteEvidence::MerchantCreated(false));
    assert_eq!(signals_for(WriteOp::UpdateTransaction, keep), &[]);
}
