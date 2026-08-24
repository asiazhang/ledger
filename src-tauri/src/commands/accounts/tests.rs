use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::AppError;
use crate::models::Account;

fn setup() -> rusqlite::Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn list_accounts(conn: &rusqlite::Connection) -> Vec<Account> {
    query_all(
        conn,
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden \
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
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0,0)",
        rusqlite::params![id, name, kind, currency, initial, now, now, 1, device_id()],
    ).unwrap();
}

fn insert_hidden_account(conn: &rusqlite::Connection, id: &str, name: &str, currency: &str) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden) \
         VALUES (?1,?2,'other',?3,0,?4,?5,?6,?7,0,1)",
        rusqlite::params![id, name, currency, now, now, 1, device_id()],
    ).unwrap();
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
        rusqlite::params![id, kind, amount, account_id, to_account_id, now, now, 1, device_id()],
    ).unwrap();
}

fn balance(conn: &rusqlite::Connection, account_id: &str) -> i64 {
    crate::db::balance::compute_balance(conn, account_id).unwrap()
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
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'bank','CNY',0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "测试账户", now, now, 1, device_id()],
    ).unwrap();
    let accounts = list_accounts(&conn);
    assert!(accounts.iter().any(|a| a.id == id && a.name == "测试账户"));
}

#[test]
fn delete_account_soft_deletes() {
    let conn = setup();
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'cash','CNY',0,?3,?4,?5,?6,0)",
        rusqlite::params![id, "待删除", now, now, 1, device_id()],
    ).unwrap();
    assert!(list_accounts(&conn).iter().any(|a| a.id == id));
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    ).unwrap();
    assert!(!list_accounts(&conn).iter().any(|a| a.id == id));
}

#[test]
fn delete_account_internal_soft_deletes_and_excludes_from_readback() {
    let conn = setup();
    insert_account(&conn, "acc-del-1", "现金", "cash", "CNY", 0);
    super::delete_account_internal(&conn, "acc-del-1").unwrap();
    assert!(
        !list_accounts(&conn).iter().any(|a| a.id == "acc-del-1"),
        "删除后不应出现在读回结果中"
    );
}

#[test]
fn delete_account_internal_returns_not_found_for_missing_id() {
    let conn = setup();
    let err = super::delete_account_internal(&conn, "不存在的id").unwrap_err();
    assert!(matches!(err, AppError::NotFound(_)));
    assert!(err.to_string().contains("账户不存在"));
}

#[test]
fn delete_account_internal_returns_not_found_for_already_deleted() {
    let conn = setup();
    insert_account(&conn, "acc-del-2", "现金", "cash", "CNY", 0);
    super::delete_account_internal(&conn, "acc-del-2").unwrap();
    let err = super::delete_account_internal(&conn, "acc-del-2").unwrap_err();
    assert!(
        matches!(err, AppError::NotFound(_)),
        "已删除账户应再次返回 404"
    );
}

#[test]
fn balance_starts_at_initial() {
    let conn = setup();
    insert_account(&conn, "acc-bal-1", "现金", "cash", "CNY", 10000);
    assert_eq!(balance(&conn, "acc-bal-1"), 10000);
}

#[test]
fn balance_adds_income() {
    let conn = setup();
    insert_account(&conn, "acc-bal-2", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx1", "income", 5000, "acc-bal-2", None);
    assert_eq!(balance(&conn, "acc-bal-2"), 5000);
}

#[test]
fn balance_subtracts_expense() {
    let conn = setup();
    insert_account(&conn, "acc-bal-3", "现金", "cash", "CNY", 10000);
    insert_tx(&conn, "tx2", "expense", 3000, "acc-bal-3", None);
    assert_eq!(balance(&conn, "acc-bal-3"), 7000);
}

#[test]
fn balance_adds_transfer_in() {
    let conn = setup();
    insert_account(&conn, "acc-a", "账户A", "cash", "CNY", 0);
    insert_account(&conn, "acc-b", "账户B", "cash", "CNY", 0);
    insert_tx(&conn, "tx3", "transfer", 2000, "acc-a", Some("acc-b"));
    assert_eq!(balance(&conn, "acc-a"), -2000);
    assert_eq!(balance(&conn, "acc-b"), 2000);
}

#[test]
fn balance_adds_refund() {
    let conn = setup();
    insert_account(&conn, "acc-bal-4", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx4", "expense", 1000, "acc-bal-4", None);
    insert_tx(&conn, "tx5", "refund", 300, "acc-bal-4", None);
    assert_eq!(balance(&conn, "acc-bal-4"), -700);
}

#[test]
fn soft_deleted_transaction_excluded_from_balance() {
    let conn = setup();
    insert_account(&conn, "acc-bal-5", "现金", "cash", "CNY", 0);
    insert_tx(&conn, "tx6", "income", 5000, "acc-bal-5", None);
    assert_eq!(balance(&conn, "acc-bal-5"), 5000);
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params!["tx6", now_iso(), device_id()],
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

    let all = crate::db::balance::compute_all_balances(&conn).unwrap();

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
        rusqlite::params!["acc-deleted", now_iso(), device_id()],
    ).unwrap();

    let all = crate::db::balance::compute_all_balances(&conn).unwrap();
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

    let accounts = super::list_accounts_for_api_internal(&conn).unwrap();
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

    let rows = crate::commands::list_transactions_internal(
        &conn,
        &crate::models::TransactionListFilter::default(),
    )
    .unwrap();
    assert!(
        rows.items
            .iter()
            .any(|t| t.id == "tx-hidden" && t.account_id == "acc-hidden"),
        "黑洞账户的交易应仍在交易列表中"
    );
}

#[test]
fn hidden_account_balance_excluded_from_all_balances() {
    let conn = setup();
    insert_hidden_account(&conn, "acc-hidden", "无(CNY)", "CNY");
    insert_tx(&conn, "tx-h", "income", 5000, "acc-hidden", None);

    let all = crate::db::balance::compute_all_balances(&conn).unwrap();
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

    let summary: crate::models::MonthlySummary = query_all(
        &conn,
        "SELECT substr(date,1,7) AS month, \
         SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END), \
         SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END), \
         SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
         FROM transactions WHERE is_deleted=0 GROUP BY month",
        [],
    )
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
