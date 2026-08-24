use crate::db::{device_id, now_iso};

fn setup() -> rusqlite::Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn first_expense_category_id(conn: &rusqlite::Connection) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE kind='expense' AND parent_id IS NULL ORDER BY created_at LIMIT 1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn first_expense_subcategory_id(conn: &rusqlite::Connection, parent_id: &str) -> String {
    conn.query_row(
        "SELECT id FROM categories WHERE parent_id=?1 ORDER BY created_at LIMIT 1",
        rusqlite::params![parent_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn insert_budget(
    conn: &rusqlite::Connection,
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

fn insert_tx(
    conn: &rusqlite::Connection,
    id: &str,
    kind: &str,
    amount_cents: i64,
    category_id: &str,
    date: &str,
) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'CNY',?3,'dummy',NULL,?4,NULL,NULL,?5,?6,?7,?8,?9,0)",
        rusqlite::params![id, kind, amount_cents, category_id, date, now, now, 1, device_id()],
    ).unwrap();
}

fn insert_dummy_account(conn: &rusqlite::Connection) {
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('dummy','虚拟账户','cash','CNY',0,?1,?2,?3,?4,0)",
        rusqlite::params![now, now, 1, device_id()],
    ).unwrap();
}

fn budget_progress(conn: &rusqlite::Connection) -> Vec<(String, String, i64, i64, bool)> {
    let mut stmt = conn
        .prepare(
            "SELECT b.id,b.category_id,b.amount_cents, \
         COALESCE((SELECT SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
                                    WHEN t.kind='refund' THEN -t.amount_native_cents \
                                    ELSE 0 END) \
                   FROM transactions t \
                   JOIN categories tc ON tc.id=t.category_id \
                   WHERE (tc.id=b.category_id OR tc.parent_id=b.category_id) \
                     AND t.is_deleted=0 \
                     AND substr(t.date,1,7)=substr(b.start_date,1,7)),0), \
         c.name \
         FROM budgets b LEFT JOIN categories c ON c.id=b.category_id \
         WHERE b.is_deleted=0 ORDER BY b.created_at",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            let amount_cents: i64 = r.get(2)?;
            let spent: i64 = r.get(3)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                amount_cents,
                spent,
                spent > amount_cents,
            ))
        })
        .unwrap();
    rows.map(|r| {
        let (id, cat_id, budget_amt, spent, over) = r.unwrap();
        (id, cat_id, budget_amt, spent, over)
    })
    .collect()
}

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
    ).unwrap();
    assert_eq!(
        conn.query_row::<i64, _, _>("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r
            .get(0))
            .unwrap(),
        0,
    );
}

#[test]
fn budget_progress_zero_when_no_transactions() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_budget(&conn, "budget-3", &cat_id, 50000, "2026-07-01");
    let results = budget_progress(&conn);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].2, 50000); // budget amount
    assert_eq!(results[0].3, 0); // spent
    assert!(!results[0].4); // not over budget
}

#[test]
fn budget_progress_counts_expense_in_same_category() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_dummy_account(&conn);
    insert_budget(&conn, "budget-4", &cat_id, 50000, "2026-07-01");
    insert_tx(&conn, "tx1", "expense", 3000, &cat_id, "2026-07-15");
    let results = budget_progress(&conn);
    assert_eq!(results[0].3, 3000);
    assert!(!results[0].4);
}

#[test]
fn budget_progress_includes_child_category_transactions() {
    let conn = setup();
    let parent_id = first_expense_category_id(&conn);
    let child_id = first_expense_subcategory_id(&conn, &parent_id);
    insert_dummy_account(&conn);
    insert_budget(&conn, "budget-5", &parent_id, 50000, "2026-07-01");
    insert_tx(&conn, "tx2", "expense", 2000, &child_id, "2026-07-10");
    let results = budget_progress(&conn);
    assert_eq!(results[0].3, 2000);
}

#[test]
fn budget_progress_refund_reduces_spent() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_dummy_account(&conn);
    insert_budget(&conn, "budget-6", &cat_id, 50000, "2026-07-01");
    insert_tx(&conn, "tx3", "expense", 5000, &cat_id, "2026-07-05");
    insert_tx(&conn, "tx4", "refund", 1000, &cat_id, "2026-07-06");
    let results = budget_progress(&conn);
    assert_eq!(results[0].3, 4000);
}

#[test]
fn budget_progress_over_budget() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_dummy_account(&conn);
    insert_budget(&conn, "budget-7", &cat_id, 1000, "2026-07-01");
    insert_tx(&conn, "tx5", "expense", 2000, &cat_id, "2026-07-10");
    let results = budget_progress(&conn);
    assert_eq!(results[0].3, 2000);
    assert!(results[0].4); // over budget
}

#[test]
fn budget_progress_only_counts_same_month() {
    let conn = setup();
    let cat_id = first_expense_category_id(&conn);
    insert_dummy_account(&conn);
    insert_budget(&conn, "budget-8", &cat_id, 50000, "2026-07-01");
    insert_tx(&conn, "tx6", "expense", 3000, &cat_id, "2026-06-30"); // previous month
    let results = budget_progress(&conn);
    assert_eq!(results[0].3, 0); // should not count
}
