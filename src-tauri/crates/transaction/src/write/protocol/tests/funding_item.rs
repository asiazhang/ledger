use ledger_sync_engine::{DomainCommand, read_ops};
use tauri_app_lib::ledger_transaction::TransactionCommand;
use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::test_support;

use crate::tests::common::make_input;

fn item(account_id: &str, amount_cents: i64, label: Option<&str>) -> TransactionFundingInput {
    TransactionFundingInput {
        account_id: account_id.into(),
        amount_cents,
        label: label.map(String::from),
    }
}

fn combined_payment_input() -> TransactionInput {
    TransactionInput {
        account_id: None,
        funding: vec![
            item("acc-wallet", 2853, None),
            item("acc-balance", 25307, None),
        ],
        ..make_input("", TransactionKind::Expense, 28160, "2026-01-01")
    }
}

fn cached(conn: &rusqlite::Connection, id: &str) -> i64 {
    conn.query_row(
        "SELECT balance_cents FROM account_balance_cache WHERE account_id=?1",
        rusqlite::params![id],
        |r| r.get(0),
    )
    .unwrap()
}

fn seed(conn: &rusqlite::Connection) {
    test_support::seed_account(conn, "acc-wallet", "小金库", "cash", "CNY", 0);
    test_support::seed_account(conn, "acc-balance", "余额", "cash", "CNY", 0);
}

#[test]
fn create_op_payload_carries_funding_breakdown() {
    let conn = test_support::open();
    seed(&conn);
    let created = tauri_app_lib::ledger_transaction::create_transaction_internal(
        &conn,
        combined_payment_input(),
    )
    .unwrap();

    let ops = read_ops(&conn).unwrap();
    assert_eq!(ops.len(), 1);
    let DomainCommand::Transaction(TransactionCommand::Create { id, row, .. }) = &ops[0].command
    else {
        panic!("应为 create 命令");
    };
    assert_eq!(id, &created.id);
    assert_eq!(row.account_id, None, "分解行主列随载荷为 None");
    assert_eq!(
        row.funding.len(),
        2,
        "出资分解随 op 搬运（Replay 形态同语义）"
    );
    assert_eq!(row.funding[0].account_id, "acc-wallet");
    assert_eq!(row.funding[1].amount_cents, 25307);
}

#[test]
fn update_single_to_multi_conversion_replaces_rows_and_unions_balances() {
    let conn = test_support::open();
    seed(&conn);
    let created = tauri_app_lib::ledger_transaction::create_transaction_internal(
        &conn,
        make_input("acc-wallet", TransactionKind::Expense, 28160, "2026-01-01"),
    )
    .unwrap();
    assert_eq!(cached(&conn, "acc-wallet"), -28160);

    tauri_app_lib::ledger_transaction::update_transaction_internal(
        &conn,
        &created.id,
        combined_payment_input(),
    )
    .unwrap();
    assert_eq!(cached(&conn, "acc-wallet"), -2853, "旧端恢复到分摊后的份额");
    assert_eq!(cached(&conn, "acc-balance"), -25307, "新出资端计入现金腿");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_fundings WHERE transaction_id=?1",
            rusqlite::params![created.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn update_multi_to_single_conversion_restores_main_account() {
    let conn = test_support::open();
    seed(&conn);
    let created = tauri_app_lib::ledger_transaction::create_transaction_internal(
        &conn,
        combined_payment_input(),
    )
    .unwrap();

    tauri_app_lib::ledger_transaction::update_transaction_internal(
        &conn,
        &created.id,
        make_input("acc-wallet", TransactionKind::Expense, 28160, "2026-01-01"),
    )
    .unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_fundings WHERE transaction_id=?1",
            rusqlite::params![created.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "分解被全量语义移除");
    let detail =
        tauri_app_lib::ledger_transaction::get_transaction_internal(&conn, &created.id).unwrap();
    assert_eq!(detail.account_id.as_deref(), Some("acc-wallet"));
    assert_eq!(
        cached(&conn, "acc-wallet"),
        -28160,
        "旧出资端 ∪ 新主端并集重算"
    );
    assert_eq!(cached(&conn, "acc-balance"), 0, "原出资端余额恢复");
}

#[test]
fn delete_funding_row_restores_all_funded_accounts() {
    let conn = test_support::open();
    seed(&conn);
    let created = tauri_app_lib::ledger_transaction::create_transaction_internal(
        &conn,
        combined_payment_input(),
    )
    .unwrap();

    tauri_app_lib::ledger_transaction::delete_transaction_internal(&conn, &created.id).unwrap();

    assert_eq!(cached(&conn, "acc-wallet"), 0, "删除恢复小金库现金腿");
    assert_eq!(cached(&conn, "acc-balance"), 0, "删除恢复余额账户现金腿");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_fundings WHERE transaction_id=?1",
            rusqlite::params![created.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "子行不随软删删除（失效由主行口径承载）");
}

#[test]
fn replay_rejects_dead_funding_item_account() {
    let conn = test_support::open();
    seed(&conn);
    let created = tauri_app_lib::ledger_transaction::create_transaction_internal(
        &conn,
        combined_payment_input(),
    )
    .unwrap();
    let ops = read_ops(&conn).unwrap();
    let DomainCommand::Transaction(TransactionCommand::Create { id, row, .. }) = &ops[0].command
    else {
        panic!("应为 create 命令");
    };

    conn.execute(
        "UPDATE accounts SET is_deleted=1 WHERE id='acc-balance'",
        [],
    )
    .unwrap();
    conn.execute("DELETE FROM transaction_fundings", [])
        .unwrap();
    conn.execute(
        "UPDATE transactions SET account_id='acc-wallet', version=version+1 WHERE id=?1",
        rusqlite::params![created.id],
    )
    .unwrap();
    let err = tauri_app_lib::ledger_transaction::replay_command(
        &conn,
        &TransactionCommand::Create {
            id: id.clone(),
            row: row.clone(),
            investment: None,
            convert: None,
            split: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code(), Some("account.not-found"), "出资端缺行码化拒绝");
}

#[test]
fn replay_rejects_funding_item_currency_mismatch() {
    let conn = test_support::open();
    seed(&conn);
    test_support::seed_account(&conn, "acc-usd", "美元户", "cash", "USD", 0);
    let _created = tauri_app_lib::ledger_transaction::create_transaction_internal(
        &conn,
        combined_payment_input(),
    )
    .unwrap();
    let ops = read_ops(&conn).unwrap();
    let DomainCommand::Transaction(TransactionCommand::Create { id, mut row, .. }) =
        ops[0].command.clone()
    else {
        panic!("应为 create 命令");
    };

    row.funding[0].account_id = "acc-usd".into();
    let err = tauri_app_lib::ledger_transaction::replay_command(
        &conn,
        &TransactionCommand::Create {
            id,
            row,
            investment: None,
            convert: None,
            split: None,
        },
    )
    .unwrap_err();
    assert_eq!(
        err.code(),
        Some("funding.currency-mismatch"),
        "出资子行币种复验码化拒绝"
    );
}
