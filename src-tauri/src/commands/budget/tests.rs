//! 预算测试（issue #58 迁移后）：断言预算核心函数与 Amount 接缝的度量矩阵一致，
//! 不复制生产 SQL——期望值由 `signed_amount`（kind × measure）对夹具逐行求和得出。

use rusqlite::Connection;

use crate::commands::budget::budget_progress_rows;
use crate::db::{device_id, now_iso};
use crate::transaction::amount::{Measure, TransactionKind, signed_amount};

fn setup() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn first_expense_category_id(conn: &Connection) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE kind='expense' AND parent_id IS NULL ORDER BY created_at LIMIT 1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn first_expense_subcategory_id(conn: &Connection, parent_id: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE parent_id=?1 ORDER BY created_at LIMIT 1",
        rusqlite::params![parent_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn insert_budget(
    conn: &Connection,
    id: &str,
    category_id: &str,
    amount_cents: i64,
    start_date: &str,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'monthly',?3,?4,?5,?6,?7,?8,0)",
        rusqlite::params![id, category_id, amount_cents, start_date, now, now, 1, device_id()],
    ).unwrap();
}

fn insert_dummy_account(conn: &Connection) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('dummy','虚拟账户','cash','CNY',0,?1,?2,?3,?4,0)",
        rusqlite::params![now, now, 1, device_id()],
    ).unwrap();
}

/// 夹具一行 = 一笔交易（kind 用 Amount 接缝的 TransactionKind 枚举表述）。
struct TxRow {
    id: &'static str,
    kind: TransactionKind,
    amount: i64,
    category_id: String,
    date: &'static str,
}

fn insert_tx(conn: &Connection, r: &TxRow) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,'dummy',NULL,?4,NULL,NULL,?5,?6,?7,1,?8,0)",
        rusqlite::params![r.id, r.kind.as_str(), r.amount, r.category_id, r.date, now, now, device_id()],
    )
    .unwrap();
}

/// 按月对夹具逐行求 `expense_net` 度量和——期望值的唯一来源是度量矩阵，不是生产 SQL。
fn expense_net_sum(rows: &[TxRow], month: &str) -> i64 {
    rows.iter()
        .filter(|r| r.date.starts_with(month))
        .map(|r| signed_amount(r.kind, r.amount, Measure::ExpenseNet))
        .sum()
}

/// 覆盖全部 8 种 kind 的夹具：验证预算 spent 恒等于 expense_net 口径。
fn all_kinds_fixture(category_id: &str) -> Vec<TxRow> {
    let category_id = category_id.to_string();
    vec![
        TxRow {
            id: "k-income",
            kind: TransactionKind::Income,
            amount: 5000,
            category_id: category_id.clone(),
            date: "2026-07-05",
        },
        TxRow {
            id: "k-expense",
            kind: TransactionKind::Expense,
            amount: 1200,
            category_id: category_id.clone(),
            date: "2026-07-06",
        },
        TxRow {
            id: "k-refund",
            kind: TransactionKind::Refund,
            amount: 300,
            category_id: category_id.clone(),
            date: "2026-07-07",
        },
        TxRow {
            id: "k-transfer",
            kind: TransactionKind::Transfer,
            amount: 800,
            category_id: category_id.clone(),
            date: "2026-07-08",
        },
        TxRow {
            id: "k-buy",
            kind: TransactionKind::Buy,
            amount: 2000,
            category_id: category_id.clone(),
            date: "2026-07-09",
        },
        TxRow {
            id: "k-sell",
            kind: TransactionKind::Sell,
            amount: 1500,
            category_id: category_id.clone(),
            date: "2026-07-10",
        },
        TxRow {
            id: "k-dividend",
            kind: TransactionKind::Dividend,
            amount: 60,
            category_id: category_id.clone(),
            date: "2026-07-11",
        },
        TxRow {
            id: "k-split",
            kind: TransactionKind::Split,
            amount: 9999,
            category_id,
            date: "2026-07-12",
        },
    ]
}

// ---- 预算 CRUD（薄壳 SQL 层） ----

#[test]
fn list_budgets_empty_initially() {
    let conn = setup();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn create_budget_and_list() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-1", &cat_id, 50000, "2026-07-01");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn delete_budget_soft_deletes() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-2", &cat_id, 50000, "2026-07-01");
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r
            .get(0))
            .unwrap(),
        1,
    );
    conn.execute(
        "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params!["budget-2", now_iso(), device_id()],
    )
    .unwrap();
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r
            .get(0))
            .unwrap(),
        0,
    );
}

// ---- budget_progress_rows：expense_net 口径 ----

#[test]
fn budget_progress_zero_when_no_transactions() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-3", &cat_id, 50000, "2026-07-01");
    let results = budget_progress_rows(&conn).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].budget.amount_cents, 50000);
    assert_eq!(results[0].spent_cents, 0);
    assert!(!results[0].over_budget);
}

/// 预算 spent = `expense_net`（毛支出 − 退款）对全部 kind 一致；
/// 投资/转账/收入/拆股类不进预算口径。
#[test]
fn budget_progress_spent_matches_expense_net_for_all_kinds() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-4", &cat_id, 50000, "2026-07-01");
    let fixture = all_kinds_fixture(&cat_id);
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].spent_cents,
        expense_net_sum(&fixture, "2026-07"),
        "spent 应为 expense_net 口径"
    );
    assert_eq!(results[0].spent_cents, 900, "1200 − 300 退款");
    assert!(!results[0].over_budget);
}

#[test]
fn budget_progress_includes_child_category_transactions() {
    let conn = setup();
    insert_dummy_account(&conn);
    let parent_id = first_expense_category_id(&conn);
    let child_id = first_expense_subcategory_id(&conn, &parent_id);
    insert_budget(&conn, "budget-5", &parent_id, 50000, "2026-07-01");
    let fixture = vec![TxRow {
        id: "tx2",
        kind: TransactionKind::Expense,
        amount: 2000,
        category_id: child_id,
        date: "2026-07-10",
    }];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn).unwrap();
    assert_eq!(
        results[0].spent_cents,
        expense_net_sum(&fixture, "2026-07"),
        "子分类交易应计入父分类预算"
    );
}

#[test]
fn budget_progress_over_budget() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-6", &cat_id, 1000, "2026-07-01");
    let fixture = vec![TxRow {
        id: "tx5",
        kind: TransactionKind::Expense,
        amount: 2000,
        category_id: cat_id,
        date: "2026-07-10",
    }];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn).unwrap();
    assert_eq!(results[0].spent_cents, expense_net_sum(&fixture, "2026-07"));
    assert!(results[0].over_budget);
}

#[test]
fn budget_progress_only_counts_same_month() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-7", &cat_id, 50000, "2026-07-01");
    let fixture = vec![TxRow {
        id: "tx6",
        kind: TransactionKind::Expense,
        amount: 3000,
        category_id: cat_id,
        date: "2026-06-30", // 上月
    }];
    for r in &fixture {
        insert_tx(&conn, r);
    }
    let results = budget_progress_rows(&conn).unwrap();
    assert_eq!(
        results[0].spent_cents,
        expense_net_sum(&fixture, "2026-07"),
        "预算月份之外的交易不计入"
    );
    assert_eq!(results[0].spent_cents, 0);
}

#[test]
fn budget_progress_excludes_deleted() {
    let conn = setup();
    insert_dummy_account(&conn);
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-8", &cat_id, 50000, "2026-07-01");
    insert_tx(
        &conn,
        &TxRow {
            id: "tx7",
            kind: TransactionKind::Expense,
            amount: 3000,
            category_id: cat_id,
            date: "2026-07-15",
        },
    );
    conn.execute("UPDATE transactions SET is_deleted=1 WHERE id='tx7'", [])
        .unwrap();
    let results = budget_progress_rows(&conn).unwrap();
    assert_eq!(results[0].spent_cents, 0);
}
