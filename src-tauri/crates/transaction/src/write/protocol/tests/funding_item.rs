//! 出资项写入协议路径（issue #1860 / ADR-0138 决策 5/6/7 + 影响节）：创建 op 载荷
//! 携分解、修改单⇄多就地互转（全量替换 + 受影响账户并集）、软删子行失效、
//! 重放存活校验扩到出资端。

use ledger_sync_engine::{DomainCommand, read_ops};
use tauri_app_lib::ledger_transaction::TransactionCommand;
use tauri_app_lib::ledger_transaction::TransactionFundingInput;
use tauri_app_lib::ledger_transaction::TransactionInput;
use tauri_app_lib::ledger_transaction::amount::TransactionKind;
use tauri_app_lib::test_support;

use crate::tests::common::make_input;

/// 分解条目构造器。
fn item(account_id: &str, amount_cents: i64, label: Option<&str>) -> TransactionFundingInput {
    TransactionFundingInput {
        account_id: account_id.into(),
        amount_cents,
        label: label.map(String::from),
    }
}

/// 组合支付入参（¥281.60 = 小金库 ¥28.53 + 余额 ¥253.07）。
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

/// 缓存余额读取便捷形态。
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

// ---------------------------------------------------------------------------
// 创建：op 载荷随分解走（同步影响节：DomainCommand 载荷随写入命令走）
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// 修改：单⇄多就地互转（全量替换语义 + 受影响账户并集整体重算）
// ---------------------------------------------------------------------------

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

    // 单 → 多：全部出资方都进受影响账户并集（旧端恢复 + 各新端计入）。
    tauri_app_lib::ledger_transaction::update_transaction_internal(
        &conn,
        &created.id,
        combined_payment_input(),
    )
    .unwrap();
    assert_eq!(cached(&conn, "acc-wallet"), -2853, "旧端恢复到分摊后的份额");
    assert_eq!(cached(&conn, "acc-balance"), -25307, "新出资端计入现金腿");

    // 子行全量替换：单出资行不再有旧形态残留。
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

    // 多 → 单：分解被整体移除、主表账户列回填。
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

// ---------------------------------------------------------------------------
// 软删：子行随主行失效（无独立软删位），余额恢复到每一出资方
// ---------------------------------------------------------------------------

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
    // 子行无独立软删位：行保留在库（主行硬删才 CASCADE），随主行 is_deleted 失效。
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transaction_fundings WHERE transaction_id=?1",
            rusqlite::params![created.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 2, "子行不随软删删除（失效由主行口径承载）");
}

// ---------------------------------------------------------------------------
// 重放：存活校验扩到出资端（影响节：缺行挂起同 ParkedOp 口径）
// ---------------------------------------------------------------------------

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

    // 出资端账户软删后重放：存活校验（扩到出资端）码化拒绝，引擎挂起待裁决。
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

/// 载荷漂移（出资项币种 ≠ 交易币种）→ 重放端币种复验拒绝（ADR-0138 影响节：
/// 「现金腿即账户币种」在子行端同样成立；存活校验先行，此处实际只补币种差）。
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

    // 首条出资项换成 USD 账户（金额不变）：本地无法产出的漂移载荷只能伪造。
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
