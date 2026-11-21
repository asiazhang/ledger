use super::model::Account;
use ledger_infra::db::query::query_all;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, ErrClass};
use ledger_sync_protocol::device::device_id;
use ledger_transaction::amount::{Measure, TransactionKind, TransferSide, signed_amount};

fn setup() -> rusqlite::Connection {
    tauri_app_lib::test_support::open()
}

fn list_accounts(conn: &rusqlite::Connection) -> Vec<Account> {
    query_all(
        conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden,is_archived,credit_limit_cents,statement_day,due_day \
         FROM accounts WHERE is_deleted=0 AND is_hidden=0 ORDER BY created_at",
        [],
    )
    .unwrap()
}

fn insert_account(
    conn: &rusqlite::Connection,
    id: &str,
    name: &str,
    kind: &str,
    currency: &str,
    initial: i64,
) {
    tauri_app_lib::test_support::seed_account(conn, id, name, kind, currency, initial);
    crate::balance::refresh_account_balances(conn, &[id]).unwrap();
}

fn insert_hidden_account(conn: &rusqlite::Connection, id: &str, name: &str, currency: &str) {
    tauri_app_lib::test_support::seed_account(conn, id, name, "other", currency, 0);
    conn.execute(
        "UPDATE accounts SET is_hidden=1 WHERE id=?1",
        rusqlite::params![id],
    )
    .unwrap();
    crate::balance::refresh_account_balances(conn, &[id]).unwrap();
}

fn insert_tx(
    conn: &rusqlite::Connection,
    id: &str,
    kind: &str,
    amount: i64,
    account_id: &str,
    to_account_id: Option<&str>,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,?4,?5,NULL,NULL,NULL,'2026-01-15',?6,?7,?8,?9,0)",
        rusqlite::params![id, kind, amount, account_id, to_account_id, now, now, 1, device_id(conn).unwrap()],
    ).unwrap();
    let mut affected = vec![account_id];
    if let Some(to) = to_account_id {
        affected.push(to);
    }
    crate::balance::refresh_account_balances(conn, &affected).unwrap();
}

fn insert_tx_dual(
    conn: &rusqlite::Connection,
    id: &str,
    kind: &str,
    amount_cents: i64,
    amount_native_cents: i64,
    account_id: &str,
    to_account_id: Option<&str>,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'HKD',?4,?5,?6,NULL,NULL,NULL,'2026-01-15',?7,?8,?9,?10,0)",
        rusqlite::params![id, kind, amount_cents, amount_native_cents, account_id, to_account_id, now, now, 1, device_id(conn).unwrap()],
    ).unwrap();
    let mut affected = vec![account_id];
    if let Some(to) = to_account_id {
        affected.push(to);
    }
    crate::balance::refresh_account_balances(conn, &affected).unwrap();
}

#[test]
fn foreign_currency_balance_accumulates_in_account_currency() {
    let conn = setup();
    insert_account(&conn, "acc-hkd", "富图牛牛", "investment", "HKD", 0);
    insert_tx_dual(&conn, "hkd-d1", "dividend", 19091, 15541, "acc-hkd", None);
    insert_tx_dual(&conn, "hkd-e1", "expense", 3000, 2442, "acc-hkd", None);
    insert_tx_dual(&conn, "hkd-e2", "expense", 1800, 1465, "acc-hkd", None);
    insert_tx_dual(&conn, "hkd-e3", "expense", 6, 5, "acc-hkd", None);

    assert_eq!(balance(&conn, "acc-hkd"), 14285, "余额应为账户币种原币之和");

    let rows = crate::balance::list_account_balances_with_visibility(&conn, false).unwrap();
    let row = rows
        .iter()
        .find(|r| r.account.id == "acc-hkd")
        .expect("账户应入清单");
    assert_eq!(row.balance_cents, 14285, "缓存出口应为账户币种余额");
}

#[test]
fn foreign_currency_black_hole_balance_stays_in_account_currency() {
    let conn = setup();
    insert_hidden_account(&conn, "hole-hkd", "无(HKD)", "HKD");
    insert_account(&conn, "acc-inv-hkd", "港美股", "investment", "HKD", 0);
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,funding_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('hkd-buy-1','buy',52508160,'HKD',42749260,'acc-inv-hkd','hole-hkd',NULL,NULL,NULL,'2026-01-15',?1,?2,1,?3,0)",
        rusqlite::params![now, now, device_id(&conn).unwrap()],
    )
    .unwrap();
    crate::balance::refresh_account_balances(&conn, &["acc-inv-hkd", "hole-hkd"]).unwrap();

    assert_eq!(
        balance(&conn, "hole-hkd"),
        -52_508_160,
        "黑洞余额应为账户币种原币金额"
    );
    assert_eq!(
        balance(&conn, "acc-inv-hkd"),
        0,
        "出资账户命中时投资账户现金腿为 0"
    );
}

fn balance(conn: &rusqlite::Connection, account_id: &str) -> i64 {
    crate::balance::compute_balance(conn, account_id).unwrap()
}

#[test]
fn list_accounts_empty_initially() {
    let conn = setup();
    let accounts = list_accounts(&conn);
    assert!(accounts.is_empty());
}

#[test]
fn create_account_and_list() {
    let conn = setup();
    let id = new_uuid();
    insert_account(&conn, &id, "测试账户", "bank", "CNY", 0);
    let accounts = list_accounts(&conn);
    assert!(accounts.iter().any(|a| a.id == id && a.name == "测试账户"));
}

#[test]
fn delete_account_soft_deletes() {
    let conn = setup();
    let id = new_uuid();
    insert_account(&conn, &id, "待删除", "cash", "CNY", 0);
    assert!(list_accounts(&conn).iter().any(|a| a.id == id));
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(&conn).unwrap()],
    ).unwrap();
    assert!(!list_accounts(&conn).iter().any(|a| a.id == id));
}

#[test]
fn delete_account_soft_deletes_and_excludes_from_readback() {
    let conn = setup();
    insert_account(&conn, "acc-del-1", "现金", "cash", "CNY", 0);
    super::delete_account(&conn, "acc-del-1").unwrap();
    assert!(
        !list_accounts(&conn).iter().any(|a| a.id == "acc-del-1"),
        "删除后不应出现在读回结果中"
    );
}

#[test]
fn delete_account_returns_not_found_for_missing_id() {
    let conn = setup();
    let err = super::delete_account(&conn, "不存在的id").unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
    assert!(err.to_string().contains("账户不存在"));
}

#[test]
fn delete_account_returns_not_found_for_already_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-del-2", "现金", "cash", "CNY", 0);
    super::delete_account(&conn, "acc-del-2").unwrap();
    let err = super::delete_account(&conn, "acc-del-2").unwrap_err();
    assert!(
        matches!(
            err,
            AppError::Coded {
                class: ErrClass::NotFound,
                ..
            }
        ),
        "已删除账户应再次返回 404"
    );
}

#[derive(Debug)]
struct ScenarioTx {
    kind: TransactionKind,
    amount: i64,
    account_id: &'static str,
    to_account_id: Option<&'static str>,
}

#[derive(Debug)]
struct ScenarioRow {
    name: &'static str,
    accounts: &'static [(&'static str, i64)],
    txs: &'static [ScenarioTx],
    expects: &'static [(&'static str, i64)],
}

#[test]
fn balance_scenario_matrix() {
    let rows: &[ScenarioRow] = &[
        ScenarioRow {
            name: "初始额起步",
            accounts: &[("acc-bal-1", 10000)],
            txs: &[],
            expects: &[("acc-bal-1", 10000)],
        },
        ScenarioRow {
            name: "收入入账",
            accounts: &[("acc-bal-2", 0)],
            txs: &[ScenarioTx {
                kind: TransactionKind::Income,
                amount: 5000,
                account_id: "acc-bal-2",
                to_account_id: None,
            }],
            expects: &[("acc-bal-2", 5000)],
        },
        ScenarioRow {
            name: "支出扣减",
            accounts: &[("acc-bal-3", 10000)],
            txs: &[ScenarioTx {
                kind: TransactionKind::Expense,
                amount: 3000,
                account_id: "acc-bal-3",
                to_account_id: None,
            }],
            expects: &[("acc-bal-3", 7000)],
        },
        ScenarioRow {
            name: "转账 A/B 双侧",
            accounts: &[("acc-a", 0), ("acc-b", 0)],
            txs: &[ScenarioTx {
                kind: TransactionKind::Transfer,
                amount: 2000,
                account_id: "acc-a",
                to_account_id: Some("acc-b"),
            }],
            expects: &[("acc-a", -2000), ("acc-b", 2000)],
        },
        ScenarioRow {
            name: "退款冲减",
            accounts: &[("acc-bal-4", 0)],
            txs: &[
                ScenarioTx {
                    kind: TransactionKind::Expense,
                    amount: 1000,
                    account_id: "acc-bal-4",
                    to_account_id: None,
                },
                ScenarioTx {
                    kind: TransactionKind::Refund,
                    amount: 300,
                    account_id: "acc-bal-4",
                    to_account_id: None,
                },
            ],
            expects: &[("acc-bal-4", -700)],
        },
    ];
    for row in rows {
        let conn = setup();
        for (id, initial) in row.accounts {
            insert_account(&conn, id, id, "cash", "CNY", *initial);
        }
        for (i, tx) in row.txs.iter().enumerate() {
            insert_tx(
                &conn,
                &format!("tx-{i}"),
                tx.kind.as_str(),
                tx.amount,
                tx.account_id,
                tx.to_account_id,
            );
        }
        for &(account, expected) in row.expects {
            let got = balance(&conn, account);
            assert_eq!(
                got, expected,
                "场景矩阵行「{}」失败: 行数据={row:?} 账户={account}",
                row.name
            );
        }
    }
}

#[test]
fn soft_deleted_transaction_excluded_from_balance() {
    let conn = setup();
    insert_account(&conn, "acc-bal-5", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx6", "income", 5000, "acc-bal-5", None);
    assert_eq!(balance(&conn, "acc-bal-5"), 5000);
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params!["tx6", now_iso(), device_id(&conn).unwrap()],
    ).unwrap();
    assert_eq!(balance(&conn, "acc-bal-5"), 0);
}

#[test]
fn list_account_balances_returns_all_accounts() {
    let conn = setup();
    insert_account(&conn, "acc-list-1", "现金", "cash", "CNY", 10000);
    insert_account(&conn, "acc-list-2", "储蓄卡", "bank", "CNY", 50000);
    insert_tx(&conn, "tx7", "income", 3000, "acc-list-1", None);
    insert_tx(&conn, "tx8", "expense", 2000, "acc-list-2", None);
    let accounts = list_accounts(&conn);
    assert_eq!(accounts.len(), 2);
    assert_eq!(balance(&conn, "acc-list-1"), 13000);
    assert_eq!(balance(&conn, "acc-list-2"), 48000);
}

struct FlowRow {
    id: &'static str,
    kind: TransactionKind,
    amount: i64,
    account_id: &'static str,
    to_account_id: Option<&'static str>,
}

#[test]
fn balance_computed_via_account_flow_measure() {
    let conn = setup();
    insert_account(&conn, "acc-flow-m", "现金", "cash", "CNY", 1000);
    insert_account(&conn, "acc-flow-n", "证券", "investment", "CNY", 0);
    let initial = |account: &str| -> i64 {
        match account {
            "acc-flow-m" => 1000,
            _ => 0,
        }
    };

    let rows = vec![
        FlowRow {
            id: "fm1",
            kind: TransactionKind::Income,
            amount: 5000,
            account_id: "acc-flow-m",
            to_account_id: None,
        },
        FlowRow {
            id: "fm2",
            kind: TransactionKind::Expense,
            amount: 1200,
            account_id: "acc-flow-m",
            to_account_id: None,
        },
        FlowRow {
            id: "fm3",
            kind: TransactionKind::Refund,
            amount: 300,
            account_id: "acc-flow-m",
            to_account_id: None,
        },
        FlowRow {
            id: "fm4",
            kind: TransactionKind::Transfer,
            amount: 800,
            account_id: "acc-flow-m",
            to_account_id: Some("acc-flow-n"),
        },
        FlowRow {
            id: "fm5",
            kind: TransactionKind::Buy,
            amount: 2000,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
        FlowRow {
            id: "fm6",
            kind: TransactionKind::Sell,
            amount: 1500,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
        FlowRow {
            id: "fm7",
            kind: TransactionKind::Dividend,
            amount: 60,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
        FlowRow {
            id: "fm8",
            kind: TransactionKind::Split,
            amount: 9999,
            account_id: "acc-flow-n",
            to_account_id: None,
        },
    ];
    for r in &rows {
        insert_tx(
            &conn,
            r.id,
            r.kind.as_str(),
            r.amount,
            r.account_id,
            r.to_account_id,
        );
    }

    let expected = |account: &str| -> i64 {
        initial(account)
            + rows
                .iter()
                .filter(|r| r.account_id == account || r.to_account_id == Some(account))
                .map(|r| {
                    let side = if r.account_id == account {
                        TransferSide::Out
                    } else {
                        TransferSide::In
                    };
                    signed_amount(r.kind, r.amount, Measure::AccountFlow(side))
                })
                .sum::<i64>()
    };

    assert_eq!(balance(&conn, "acc-flow-m"), 4300);
    assert_eq!(balance(&conn, "acc-flow-n"), 360);

    let all = crate::balance::compute_all_balances(&conn).unwrap();
    for id in ["acc-flow-m", "acc-flow-n"] {
        assert_eq!(
            *all.get(id).unwrap_or(&0),
            expected(id),
            "批量余额与度量不一致: {id}"
        );
        assert_eq!(
            balance(&conn, id),
            expected(id),
            "单个余额与度量不一致: {id}"
        );
    }
}

#[test]
fn compute_all_balances_matches_per_account() {
    let conn = setup();
    insert_account(&conn, "acc-bulk-1", "现金", "cash", "CNY", 10000);
    insert_account(&conn, "acc-bulk-2", "储蓄卡", "bank", "CNY", 50000);
    insert_account(&conn, "acc-bulk-3", "信用卡", "credit", "CNY", 0);
    insert_tx(&conn, "tx-b1", "income", 5000, "acc-bulk-1", None);
    insert_tx(&conn, "tx-b2", "expense", 2000, "acc-bulk-1", None);
    insert_tx(&conn, "tx-b3", "expense", 1500, "acc-bulk-2", None);
    insert_tx(
        &conn,
        "tx-b4",
        "transfer",
        3000,
        "acc-bulk-1",
        Some("acc-bulk-2"),
    );
    insert_tx(&conn, "tx-b5", "refund", 500, "acc-bulk-1", None);
    insert_tx(&conn, "tx-b6", "dividend", 60, "acc-bulk-3", None);

    let all = crate::balance::compute_all_balances(&conn).unwrap();

    for id in ["acc-bulk-1", "acc-bulk-2", "acc-bulk-3"] {
        let expected = balance(&conn, id);
        let got = *all.get(id).unwrap_or(&0);
        assert_eq!(
            got, expected,
            "余额不一致: {id}, 期望 {expected}, 得到 {got}"
        );
    }
}

#[test]
fn compute_all_balances_excludes_soft_deleted_accounts() {
    let conn = setup();
    insert_account(&conn, "acc-active", "活动账户", "cash", "CNY", 1000);
    insert_account(&conn, "acc-deleted", "已删除", "cash", "CNY", 2000);
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params!["acc-deleted", now_iso(), device_id(&conn).unwrap()],
    ).unwrap();

    let all = crate::balance::compute_all_balances(&conn).unwrap();
    assert!(all.contains_key("acc-active"), "应包含活动账户");
    assert!(!all.contains_key("acc-deleted"), "不应包含已删除账户");
}

#[test]
fn list_accounts_excludes_hidden_accounts() {
    let conn = setup();
    insert_account(&conn, "acc-normal", "现金", "cash", "CNY", 0);
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");

    let accounts = list_accounts(&conn);
    assert!(
        accounts.iter().any(|a| a.id == "acc-normal"),
        "应包含普通账户"
    );
    assert!(
        !accounts.iter().any(|a| a.id == "acc-hidden"),
        "不应包含黑洞账户"
    );
}

#[test]
fn list_accounts_for_api_includes_hidden_with_flag() {
    let conn = setup();
    insert_account(&conn, "acc-normal", "现金", "cash", "CNY", 0);
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");

    let accounts = super::list_accounts_for_api(&conn).unwrap();
    let hidden = accounts.iter().find(|a| a.id == "acc-hidden").unwrap();
    assert!(hidden.is_hidden, "API 应返回 is_hidden=true 的黑洞账户");
    let normal = accounts.iter().find(|a| a.id == "acc-normal").unwrap();
    assert!(!normal.is_hidden);
}

#[test]
fn hidden_account_transaction_visible_in_transaction_list() {
    let conn = setup();
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");
    insert_tx(&conn, "tx-hidden", "expense", 3000, "acc-hidden", None);

    let rows = ledger_transaction::list_transactions_internal(
        &conn,
        &ledger_transaction::TransactionListFilter::default(),
    )
    .unwrap();
    assert!(
        rows.items
            .iter()
            .any(|t| t.id == "tx-hidden" && t.account_id.as_deref() == Some("acc-hidden")),
        "黑洞账户的交易应仍在交易列表中"
    );
}

#[test]
fn hidden_account_balance_excluded_from_all_balances() {
    let conn = setup();
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");
    insert_tx(&conn, "tx-h", "income", 5000, "acc-hidden", None);

    let all = crate::balance::compute_all_balances(&conn).unwrap();
    assert!(
        !all.contains_key("acc-hidden"),
        "compute_all_balances 不应包含黑洞账户"
    );
}

#[test]
fn hidden_account_transactions_included_in_reports() {
    let conn = setup();
    insert_account(&conn, "acc-normal", "现金", "cash", "CNY", 0);
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");
    insert_tx(&conn, "tx-normal", "income", 1000, "acc-normal", None);
    insert_tx(&conn, "tx-hidden", "expense", 2000, "acc-hidden", None);

    let summary = ledger_reports::monthly_summary_rows(&conn, 2026, None, None)
        .unwrap()
        .remove(0);
    assert_eq!(summary.income_cents, 1000);
    assert_eq!(summary.expense_cents, 2000, "黑洞账户的支出应计入报表");
}

#[test]
fn seed_contains_black_hole_accounts_for_cny_and_hkd() {
    let conn = setup();
    let mut stmt = conn
        .prepare(
            "SELECT name, currency_code, is_hidden FROM accounts WHERE is_hidden=1 ORDER BY currency_code",
        )
        .unwrap();
    let rows: Vec<(String, String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(
        rows,
        vec![
            ("无(CNY)".to_string(), "CNY".to_string(), 1),
            ("无(HKD)".to_string(), "HKD".to_string(), 1),
        ],
        "种子应预置 CNY/HKD 两个黑洞账户"
    );
}

use super::model::{AccountBalanceAdjustInput, AccountInput, AccountType, AccountUpdateInput};

fn find_black_hole(conn: &rusqlite::Connection, currency: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM accounts WHERE is_deleted=0 AND is_hidden=1 AND currency_code=?1 LIMIT 1",
        rusqlite::params![currency],
        |r| r.get(0),
    )
    .ok()
}

#[test]
fn update_account_renames_and_bumps_version() {
    let conn = setup();
    insert_account(&conn, "acc-u1", "旧名", "cash", "CNY", 0);
    super::update_account(
        &conn,
        "acc-u1",
        AccountUpdateInput {
            name: Some("新名".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap();
    let account = super::get_account(&conn, "acc-u1").unwrap();
    assert_eq!(account.name, "新名");
    assert_eq!(account.currency_code, "CNY", "未传字段保持原值");
    assert_eq!(account.version, 2, "编辑应递增 version");
}

#[test]
fn update_account_rejects_empty_name() {
    let conn = setup();
    insert_account(&conn, "acc-u2", "现金", "cash", "CNY", 0);
    let err = super::update_account(
        &conn,
        "acc-u2",
        AccountUpdateInput {
            name: Some("   ".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("名称不能为空"));
}

#[test]
fn update_account_rejects_currency_change_when_has_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-u3", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx-u3", "income", 1000, "acc-u3", None);
    let err = super::update_account(
        &conn,
        "acc-u3",
        AccountUpdateInput {
            name: None,
            currency_code: Some("HKD".into()),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("不能修改币种"));
    assert_eq!(
        super::get_account(&conn, "acc-u3").unwrap().currency_code,
        "CNY"
    );
}

#[test]
fn update_account_allows_currency_change_without_transactions() {
    let conn = setup();
    insert_account(&conn, "acc-u4", "现金", "cash", "CNY", 0);
    super::update_account(
        &conn,
        "acc-u4",
        AccountUpdateInput {
            name: None,
            currency_code: Some("HKD".into()),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap();
    assert_eq!(
        super::get_account(&conn, "acc-u4").unwrap().currency_code,
        "HKD"
    );
}

#[test]
fn update_account_rejects_unknown_currency() {
    let conn = setup();
    insert_account(&conn, "acc-u5", "现金", "cash", "CNY", 0);
    let err = super::update_account(
        &conn,
        "acc-u5",
        AccountUpdateInput {
            name: None,
            currency_code: Some("XYZ".into()),
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("未知币种"));
}

#[test]
fn update_account_returns_not_found_for_missing_id() {
    let conn = setup();
    let err = super::update_account(
        &conn,
        "不存在的id",
        AccountUpdateInput {
            name: Some("任意".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

fn adjust(conn: &rusqlite::Connection, id: &str, target: i64) -> Result<(String, bool), AppError> {
    super::adjust_account_balance(
        conn,
        id,
        &AccountBalanceAdjustInput {
            target_balance_cents: target,
            date: "2026-09-15".into(),
            note: None,
        },
    )
}

fn ensure_rate(conn: &rusqlite::Connection, code: &str, rate: f64) {
    conn.execute(
        "INSERT OR IGNORE INTO currencies (code, name, symbol, decimal_places) VALUES (?1, ?1, '$', 2)",
        rusqlite::params![code],
    )
    .unwrap();
    tauri_app_lib::test_support::seed_exchange_rate(conn, code, "CNY", rate);
    tauri_app_lib::test_support::seed_fx_rate_history(
        conn,
        &format!("fxh-adjust-{code}"),
        code,
        "CNY",
        "2026-09-15",
        rate,
    );
}

#[test]
fn adjust_up_creates_transfer_from_black_hole_and_reaches_target() {
    let conn = setup();
    insert_account(&conn, "acc-adj-1", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx-adj-1", "income", 5000, "acc-adj-1", None);
    assert_eq!(balance(&conn, "acc-adj-1"), 5000);

    let (tx_id, created) = adjust(&conn, "acc-adj-1", 7000).unwrap();
    assert!(!created, "种子已预置 无(CNY)，不应新建黑洞账户");
    let black_hole = find_black_hole(&conn, "CNY").unwrap();
    let (kind, amount, account_id, to_account_id, note): (
        String,
        i64,
        String,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT kind, amount_cents, account_id, to_account_id, note FROM transactions WHERE id=?1",
            rusqlite::params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(kind, "transfer");
    assert_eq!(amount, 2000);
    assert_eq!(account_id, black_hole, "Δ>0 应从黑洞账户转出");
    assert_eq!(to_account_id.as_deref(), Some("acc-adj-1"));
    assert_eq!(note.as_deref(), Some("余额调整"), "缺省备注为「余额调整」");
    assert_eq!(balance(&conn, "acc-adj-1"), 7000, "调整后余额应等于目标值");
}

#[test]
fn adjust_down_creates_transfer_to_black_hole() {
    let conn = setup();
    insert_account(&conn, "acc-adj-2", "现金", "cash", "CNY", 5000);
    let (tx_id, created) = adjust(&conn, "acc-adj-2", 4000).unwrap();
    assert!(!created);
    let black_hole = find_black_hole(&conn, "CNY").unwrap();
    let (account_id, to_account_id, amount): (String, Option<String>, i64) = conn
        .query_row(
            "SELECT account_id, to_account_id, amount_cents FROM transactions WHERE id=?1",
            rusqlite::params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(amount, 1000, "Δ<0 转账金额为差额绝对值");
    assert_eq!(account_id, "acc-adj-2", "Δ<0 应从目标账户转出");
    assert_eq!(to_account_id.as_deref(), Some(black_hole.as_str()));
    assert_eq!(balance(&conn, "acc-adj-2"), 4000);
}

#[test]
fn adjust_creates_black_hole_for_missing_currency() {
    let conn = setup();
    ensure_rate(&conn, "USD", 1.0);
    insert_account(&conn, "acc-adj-3", "美元户", "cash", "USD", 0);
    let (_tx_id, created) = adjust(&conn, "acc-adj-3", 9900).unwrap();
    assert!(created, "缺失币种的黑洞账户应按需自动创建");
    let black_hole = find_black_hole(&conn, "USD").unwrap();
    let (name, kind, is_hidden): (String, String, i64) = conn
        .query_row(
            "SELECT name, type, is_hidden FROM accounts WHERE id=?1",
            rusqlite::params![black_hole],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(name, "无(USD)", "自动建的黑洞账户与种子同形");
    assert_eq!(kind, "other");
    assert_eq!(is_hidden, 1);
    assert_eq!(balance(&conn, "acc-adj-3"), 9900);
}

#[test]
fn adjust_reuses_existing_black_hole_for_same_currency() {
    let conn = setup();
    ensure_rate(&conn, "HKD", 1.0);
    insert_account(&conn, "acc-adj-4", "港币户", "cash", "HKD", 0);
    let _ = adjust(&conn, "acc-adj-4", 100).unwrap();
    let (_tx_id2, created2) = adjust(&conn, "acc-adj-4", 300).unwrap();
    assert!(!created2, "第二次调整应复用已有黑洞账户");
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE is_hidden=1 AND currency_code='HKD'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1, "同币种黑洞账户不应重复创建");
    assert_eq!(balance(&conn, "acc-adj-4"), 300);
    let txs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transactions WHERE (account_id=(\
             SELECT id FROM accounts WHERE is_hidden=1 AND currency_code='HKD' LIMIT 1\
             ) OR to_account_id=(\
             SELECT id FROM accounts WHERE is_hidden=1 AND currency_code='HKD' LIMIT 1\
             )) AND is_deleted=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(txs >= 2);
}

#[test]
fn adjust_zero_delta_errors_without_writing() {
    let conn = setup();
    insert_account(&conn, "acc-adj-5", "现金", "cash", "CNY", 12345);
    let err = adjust(&conn, "acc-adj-5", 12345).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("无需调整"));
    let tx_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tx_count, 0, "零差额不应产生任何写入");
}

#[test]
fn adjust_rejects_hidden_account() {
    let conn = setup();
    insert_hidden_account(&conn, "acc-adj-6", "无(CNY)", "CNY");
    let err = adjust(&conn, "acc-adj-6", 100).unwrap_err();
    assert!(matches!(err, AppError::Coded { .. }));
    assert!(err.to_string().contains("黑洞账户不支持余额调整"));
}

#[test]
fn adjust_returns_not_found_for_missing_account() {
    let conn = setup();
    let err = adjust(&conn, "不存在的id", 100).unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
}

#[test]
fn adjust_deleted_adjustment_tx_reverts_balance() {
    let conn = setup();
    insert_account(&conn, "acc-adj-7", "现金", "cash", "CNY", 0);
    let (tx_id, _) = adjust(&conn, "acc-adj-7", 5000).unwrap();
    assert_eq!(balance(&conn, "acc-adj-7"), 5000);
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![tx_id, now_iso(), device_id(&conn).unwrap()],
    )
    .unwrap();
    assert_eq!(balance(&conn, "acc-adj-7"), 0, "删除调整交易即恢复原余额");
}

fn credit_input(name: &str) -> AccountInput {
    AccountInput {
        name: name.into(),
        kind: AccountType::Credit,
        currency_code: "CNY".into(),
        initial_balance_cents: None,
        credit_limit_cents: None,
        statement_day: None,
        due_day: None,
    }
}

#[test]
fn create_credit_account_persists_credit_terms() {
    let conn = setup();
    let id = super::create_account(
        &conn,
        AccountInput {
            credit_limit_cents: Some(5_000_000),
            statement_day: Some(31),
            due_day: Some(1),
            ..credit_input("招行信用卡")
        },
    )
    .unwrap();
    let account = super::get_account(&conn, &id).unwrap();
    assert_eq!(account.credit_limit_cents, Some(5_000_000));
    assert_eq!(
        account.statement_day,
        Some(31),
        "声明值原样存储（不做月末钳制）"
    );
    assert_eq!(
        account.due_day,
        Some(1),
        "账单日与还款日不强制先后（跨月形态合法）"
    );
}

#[test]
fn create_credit_account_allows_partial_credit_terms() {
    let conn = setup();
    let only_limit = super::create_account(
        &conn,
        AccountInput {
            credit_limit_cents: Some(1_000_000),
            ..credit_input("只填额度")
        },
    )
    .unwrap();
    let account = super::get_account(&conn, &only_limit).unwrap();
    assert_eq!(account.credit_limit_cents, Some(1_000_000));
    assert_eq!(account.statement_day, None);
    assert_eq!(account.due_day, None);

    let only_days = super::create_account(
        &conn,
        AccountInput {
            statement_day: Some(5),
            due_day: Some(25),
            ..credit_input("只填日子")
        },
    )
    .unwrap();
    let account = super::get_account(&conn, &only_days).unwrap();
    assert_eq!(account.credit_limit_cents, None, "额度未填不报错");
    assert_eq!(account.statement_day, Some(5));
    assert_eq!(account.due_day, Some(25));
}

#[test]
fn create_non_credit_account_rejects_credit_terms() {
    let conn = setup();
    for (kind, name) in [
        (AccountType::Cash, "现金"),
        (AccountType::Bank, "银行卡"),
        (AccountType::Debt, "负债"),
        (AccountType::Receivable, "借出款"),
    ] {
        let err = super::create_account(
            &conn,
            AccountInput {
                name: name.into(),
                kind,
                credit_limit_cents: Some(1_000_000),
                ..credit_input(name)
            },
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                AppError::Coded {
                    class: ErrClass::Invalid,
                    ..
                }
            ),
            "{kind} 携带信用额度应被拒绝"
        );
        assert!(err.to_string().contains("仅信用卡账户可填写"));
    }
    let names: Vec<String> = super::list_accounts_for_api(&conn)
        .unwrap()
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert!(
        !names
            .iter()
            .any(|n| n == "现金" || n == "银行卡" || n == "负债" || n == "借出款"),
        "被拒绝的创建不落库"
    );
}

#[test]
fn credit_limit_must_be_positive() {
    let conn = setup();
    for limit in [0, -1, -10_000] {
        let err = super::create_account(
            &conn,
            AccountInput {
                credit_limit_cents: Some(limit),
                ..credit_input("额度非法")
            },
        )
        .unwrap_err();
        assert!(matches!(
            err,
            AppError::Coded {
                class: ErrClass::Invalid,
                ..
            }
        ));
        assert!(err.to_string().contains("信用额度必须大于 0"));
    }
}

#[test]
fn credit_days_must_be_within_1_to_31() {
    let conn = setup();
    for day in [0, 32, -1] {
        let err = super::create_account(
            &conn,
            AccountInput {
                statement_day: Some(day),
                ..credit_input("账单日非法")
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("账单日必须在 1 到 31 之间"));
        assert!(
            err.to_string().contains(&day.to_string()),
            "越界值随消息带出"
        );

        let err = super::create_account(
            &conn,
            AccountInput {
                due_day: Some(day),
                ..credit_input("还款日非法")
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("还款日必须在 1 到 31 之间"));
    }
}

#[test]
fn update_credit_terms_supports_set_keep_and_clear() {
    let conn = setup();
    let id = super::create_account(
        &conn,
        AccountInput {
            credit_limit_cents: Some(1_000_000),
            statement_day: Some(5),
            due_day: Some(25),
            ..credit_input("信用卡")
        },
    )
    .unwrap();
    let terms = |conn: &rusqlite::Connection| {
        let a = super::get_account(conn, &id).unwrap();
        (a.credit_limit_cents, a.statement_day, a.due_day)
    };

    super::update_account(
        &conn,
        &id,
        AccountUpdateInput {
            name: Some("信用卡Ⅰ".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap();
    assert_eq!(terms(&conn), (Some(1_000_000), Some(5), Some(25)));

    super::update_account(
        &conn,
        &id,
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: Some(Some(2_000_000)),
            statement_day: Some(Some(10)),
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap();
    assert_eq!(terms(&conn), (Some(2_000_000), Some(10), Some(25)));

    super::update_account(
        &conn,
        &id,
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: Some(None),
            statement_day: None,
            due_day: Some(None),
            is_archived: None,
        },
    )
    .unwrap();
    assert_eq!(terms(&conn), (None, Some(10), None));
}

#[test]
fn update_rejects_invalid_credit_terms_without_writing() {
    let conn = setup();
    let id = super::create_account(
        &conn,
        AccountInput {
            credit_limit_cents: Some(1_000_000),
            statement_day: Some(5),
            due_day: Some(25),
            ..credit_input("信用卡")
        },
    )
    .unwrap();
    let err = super::update_account(
        &conn,
        &id,
        AccountUpdateInput {
            name: Some("改了个名".into()),
            currency_code: None,
            credit_limit_cents: Some(Some(0)),
            statement_day: Some(Some(99)),
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::Invalid,
            ..
        }
    ));
    let account = super::get_account(&conn, &id).unwrap();
    assert_eq!(account.name, "信用卡", "非法编辑不落名字等任何字段");
    assert_eq!(
        (account.credit_limit_cents, account.statement_day),
        (Some(1_000_000), Some(5))
    );
}

#[test]
fn update_non_credit_account_rejects_credit_terms() {
    let conn = setup();
    insert_account(&conn, "acc-c-9", "现金", "cash", "CNY", 0);
    let err = super::update_account(
        &conn,
        "acc-c-9",
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: None,
            statement_day: Some(Some(5)),
            due_day: None,
            is_archived: None,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("仅信用卡账户可填写"));
}

#[test]
fn list_accounts_carries_credit_terms() {
    let conn = setup();
    let id = super::create_account(
        &conn,
        AccountInput {
            credit_limit_cents: Some(5_000_000),
            statement_day: Some(5),
            due_day: Some(25),
            ..credit_input("招行信用卡")
        },
    )
    .unwrap();
    let listed = super::list_accounts(&conn)
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .expect("新建账户应在列表内");
    assert_eq!(listed.credit_limit_cents, Some(5_000_000));
    assert_eq!(listed.statement_day, Some(5));
    assert_eq!(listed.due_day, Some(25));

    let balances = crate::balance::list_account_balances_with_visibility(&conn, false).unwrap();
    let row = balances
        .into_iter()
        .find(|b| b.account.id == id)
        .expect("新建账户应在余额清单内");
    assert_eq!(row.account.credit_limit_cents, Some(5_000_000));
}

#[test]
fn credit_terms_do_not_change_account_balance() {
    let conn = setup();
    let id = super::create_account(
        &conn,
        AccountInput {
            initial_balance_cents: Some(-20_000),
            ..credit_input("招行信用卡")
        },
    )
    .unwrap();
    let before = balance(&conn, &id);
    assert_eq!(before, -20_000, "建账既有欠款经期初余额（负值）表达");

    super::update_account(
        &conn,
        &id,
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: Some(Some(5_000_000)),
            statement_day: Some(Some(5)),
            due_day: Some(Some(25)),
            is_archived: None,
        },
    )
    .unwrap();
    assert_eq!(balance(&conn, &id), before, "补填额度不改变余额");

    super::update_account(
        &conn,
        &id,
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: Some(None),
            statement_day: Some(None),
            due_day: Some(None),
            is_archived: None,
        },
    )
    .unwrap();
    assert_eq!(balance(&conn, &id), before, "清空额度不改变余额");
    assert_eq!(
        crate::balance::cached_balance(&conn, &id).unwrap(),
        before,
        "缓存口径与实时口径同值"
    );
}

fn archived_flag(conn: &rusqlite::Connection, id: &str) -> bool {
    conn.query_row(
        "SELECT is_archived FROM accounts WHERE id=?1",
        rusqlite::params![id],
        |r| r.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .unwrap()
}

fn op_count(conn: &rusqlite::Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM sync.sync_ops", [], |r| r.get(0))
        .unwrap()
}

fn op_payloads(conn: &rusqlite::Connection) -> Vec<serde_json::Value> {
    let mut stmt = conn
        .prepare("SELECT payload FROM sync.sync_ops ORDER BY clock")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect()
}

#[test]
fn archive_and_restore_roundtrip_flips_flag_and_bumps_version() {
    let conn = setup();
    insert_account(&conn, "acc-arc-1", "借出·张三", "receivable", "CNY", 0);

    super::set_account_archived(&conn, "acc-arc-1", true).unwrap();
    let archived = super::get_account(&conn, "acc-arc-1").unwrap();
    assert!(archived.is_archived, "归档后标志应为 true");
    assert_eq!(archived.version, 2, "归档是一次账户写入，递增 version");

    super::set_account_archived(&conn, "acc-arc-1", false).unwrap();
    let restored = super::get_account(&conn, "acc-arc-1").unwrap();
    assert!(!restored.is_archived, "恢复后标志应回到 false");
}

#[test]
fn archive_is_idempotent_no_second_op_no_version_bump() {
    let conn = setup();
    insert_account(&conn, "acc-arc-2", "借出·李四", "receivable", "CNY", 0);
    super::set_account_archived(&conn, "acc-arc-2", true).unwrap();
    let ops_after_first = op_count(&conn);
    let version_after_first = super::get_account(&conn, "acc-arc-2").unwrap().version;

    super::set_account_archived(&conn, "acc-arc-2", true).unwrap();
    assert_eq!(
        super::get_account(&conn, "acc-arc-2").unwrap().version,
        version_after_first,
        "已是目标态时不得再 bump version（零 UPDATE）"
    );
    assert_eq!(op_count(&conn), ops_after_first, "已是目标态时不得再产 op");
}

#[test]
fn archive_keeps_balance_totals_and_net_worth_unchanged() {
    let conn = setup();
    insert_account(&conn, "acc-arc-3", "现金", "cash", "CNY", 0);
    insert_account(&conn, "acc-arc-4", "借出·王五", "receivable", "CNY", 0);
    insert_tx(&conn, "tx-arc-3a", "income", 100_000, "acc-arc-3", None);
    insert_tx(
        &conn,
        "tx-arc-4a",
        "transfer",
        30_000,
        "acc-arc-3",
        Some("acc-arc-4"),
    );

    let balance_before = crate::balance::cached_balance(&conn, "acc-arc-4").unwrap();
    let totals_before: i64 = crate::balance::list_account_balances_with_visibility(&conn, false)
        .unwrap()
        .iter()
        .map(|row| row.balance_cents)
        .sum();
    let net_worth_before =
        tauri_app_lib::ledger_dashboard::net_worth::query_dashboard_overview(&conn)
            .unwrap()
            .net_worth_cents;

    super::set_account_archived(&conn, "acc-arc-4", true).unwrap();

    assert_eq!(
        crate::balance::cached_balance(&conn, "acc-arc-4").unwrap(),
        balance_before,
        "归档不改该账户余额"
    );
    let totals_after: i64 = crate::balance::list_account_balances_with_visibility(&conn, false)
        .unwrap()
        .iter()
        .map(|row| row.balance_cents)
        .sum();
    assert_eq!(totals_after, totals_before, "归档不改全部账户余额合计");
    let net_worth_after =
        tauri_app_lib::ledger_dashboard::net_worth::query_dashboard_overview(&conn)
            .unwrap()
            .net_worth_cents;
    assert_eq!(net_worth_after, net_worth_before, "归档不改净资产读数");
}

#[test]
fn archive_allows_nonzero_balance() {
    let conn = setup();
    insert_account(&conn, "acc-arc-5", "借出·赵六", "receivable", "CNY", 0);
    insert_tx(&conn, "tx-arc-5a", "income", 50_000, "acc-arc-5", None);
    assert_eq!(
        crate::balance::cached_balance(&conn, "acc-arc-5").unwrap(),
        50_000
    );

    super::set_account_archived(&conn, "acc-arc-5", true).unwrap();
    assert!(archived_flag(&conn, "acc-arc-5"), "余额非零允许归档");
}

#[test]
fn set_account_archived_returns_not_found_for_missing_id() {
    let conn = setup();
    let err = super::set_account_archived(&conn, "不存在的id", true).unwrap_err();
    assert!(matches!(
        err,
        AppError::Coded {
            class: ErrClass::NotFound,
            ..
        }
    ));
    assert!(err.to_string().contains("账户不存在"));
}

#[test]
fn archived_account_stays_in_read_faces_with_flag() {
    let conn = setup();
    insert_account(&conn, "acc-arc-6", "借出·钱七", "receivable", "CNY", 0);
    super::set_account_archived(&conn, "acc-arc-6", true).unwrap();

    let listed = list_accounts(&conn);
    let row = listed
        .iter()
        .find(|a| a.id == "acc-arc-6")
        .expect("列表读面不过滤归档账户");
    assert!(row.is_archived, "列表读回携带归档标志");

    let balances = crate::balance::list_account_balances_with_visibility(&conn, false).unwrap();
    assert!(
        balances.iter().any(|b| b.account.id == "acc-arc-6"),
        "余额清单不过滤归档账户"
    );
}

#[test]
fn replay_set_archived_matches_local_protocol_without_op() {
    let conn = setup();
    insert_account(&conn, "acc-arc-7", "借出·孙八", "receivable", "CNY", 0);

    crate::core::replay_set_archived(&conn, "acc-arc-7", true).unwrap();
    let replayed = super::get_account(&conn, "acc-arc-7").unwrap();
    assert!(replayed.is_archived, "重放后标志落定");
    assert_eq!(replayed.version, 2, "重放走同一写协议（bump version）");
    assert_eq!(op_count(&conn), 0, "重放不产出 op");

    crate::core::replay_set_archived(&conn, "acc-arc-7", true).unwrap();
    assert_eq!(
        super::get_account(&conn, "acc-arc-7").unwrap().version,
        2,
        "重放幂等：已目标态不再 bump"
    );
}

#[test]
fn update_account_resolves_archive_key_without_putting_it_in_update_payload() {
    let conn = setup();
    insert_account(&conn, "acc-arc-8", "借出·周九", "receivable", "CNY", 0);

    super::update_account(
        &conn,
        "acc-arc-8",
        AccountUpdateInput {
            name: Some("借出·周九（已收回）".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: Some(true),
        },
    )
    .unwrap();

    let archived = super::get_account(&conn, "acc-arc-8").unwrap();
    assert!(archived.is_archived, "编辑载荷携带归档键即落定归档");
    assert_eq!(
        archived.name, "借出·周九（已收回）",
        "改名与归档同一事务落定"
    );

    let payloads = op_payloads(&conn);
    assert_eq!(
        payloads.len(),
        2,
        "Update + SetArchived 各一条：{payloads:?}"
    );
    assert_eq!(payloads[0]["entity"], "account");
    let update = &payloads[0]["payload"];
    assert_eq!(update["action"], "update", "首条仍是编辑 op");
    assert!(
        update.get("archived").is_none() && update.get("is_archived").is_none(),
        "Update 载荷不携带归档态（旧日志重放不刷回未归档）：{update}"
    );
    assert_eq!(payloads[1]["payload"]["action"], "set_archived");
    assert_eq!(payloads[1]["payload"]["archived"], true);

    super::update_account(
        &conn,
        "acc-arc-8",
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: Some(false),
        },
    )
    .unwrap();
    assert!(
        !super::get_account(&conn, "acc-arc-8").unwrap().is_archived,
        "恢复落定"
    );
}

#[test]
fn update_account_archive_only_emits_single_set_archived_op() {
    let conn = setup();
    insert_account(&conn, "acc-arc-9", "借出·吴十", "receivable", "CNY", 0);
    let version_before = super::get_account(&conn, "acc-arc-9").unwrap().version;
    let ops_before = op_count(&conn);

    super::update_account(
        &conn,
        "acc-arc-9",
        AccountUpdateInput {
            name: None,
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: Some(true),
        },
    )
    .unwrap();

    let archived = super::get_account(&conn, "acc-arc-9").unwrap();
    assert!(archived.is_archived, "归档键落定");
    assert_eq!(
        archived.version,
        version_before + 1,
        "归档-only 编辑只写一次账户行（不先走一遍编辑路径）"
    );

    let payloads = op_payloads(&conn);
    assert_eq!(
        payloads.len() as i64,
        ops_before + 1,
        "归档-only 编辑只产一条 op：{payloads:?}"
    );
    let set_archived: Vec<&serde_json::Value> = payloads
        .iter()
        .filter(|p| p["payload"]["action"] == "set_archived")
        .collect();
    assert_eq!(set_archived.len(), 1, "恰一条 SetArchived op：{payloads:?}");
    assert_eq!(set_archived[0]["payload"]["archived"], true);
    assert!(
        !payloads.iter().any(|p| p["payload"]["action"] == "update"),
        "无编辑键时不得产 Update op：{payloads:?}"
    );
}

#[test]
fn update_account_archive_only_is_idempotent() {
    let conn = setup();
    insert_account(&conn, "acc-arc-10", "借出·郑十一", "receivable", "CNY", 0);
    let input = || AccountUpdateInput {
        name: None,
        currency_code: None,
        credit_limit_cents: None,
        statement_day: None,
        due_day: None,
        is_archived: Some(true),
    };
    super::update_account(&conn, "acc-arc-10", input()).unwrap();
    let version_after_first = super::get_account(&conn, "acc-arc-10").unwrap().version;
    let ops_after_first = op_count(&conn);

    super::update_account(&conn, "acc-arc-10", input()).unwrap();
    assert_eq!(
        super::get_account(&conn, "acc-arc-10").unwrap().version,
        version_after_first,
        "重复归档-only 编辑不得再写账户行"
    );
    assert_eq!(
        op_count(&conn),
        ops_after_first,
        "重复归档-only 编辑不得再产 op"
    );
}

#[test]
fn update_account_with_edit_and_archive_emits_both_ops() {
    let conn = setup();
    insert_account(&conn, "acc-arc-11", "借出·冯十二", "receivable", "CNY", 0);
    let version_before = super::get_account(&conn, "acc-arc-11").unwrap().version;

    super::update_account(
        &conn,
        "acc-arc-11",
        AccountUpdateInput {
            name: Some("借出·冯十二（已收回）".into()),
            currency_code: None,
            credit_limit_cents: None,
            statement_day: None,
            due_day: None,
            is_archived: Some(true),
        },
    )
    .unwrap();

    let updated = super::get_account(&conn, "acc-arc-11").unwrap();
    assert_eq!(updated.name, "借出·冯十二（已收回）");
    assert!(updated.is_archived);
    assert_eq!(
        updated.version,
        version_before + 2,
        "改名与归档各写一次账户行"
    );
    let actions: Vec<String> = op_payloads(&conn)
        .iter()
        .map(|p| {
            p["payload"]["action"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(actions, vec!["update", "set_archived"], "两条 op 各一条");
}
