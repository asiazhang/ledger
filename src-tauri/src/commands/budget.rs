use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Budget, BudgetInput, BudgetPeriod, BudgetProgress};

#[tauri::command]
pub fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted \
         FROM budgets WHERE is_deleted=0 ORDER BY created_at",
        [],
    )
}

#[tauri::command]
pub fn create_budget(db: State<'_, DbState>, input: BudgetInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    let period = input.period.unwrap_or(BudgetPeriod::Monthly).to_string();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![
            id,
            input.category_id,
            period,
            input.amount_cents,
            input.start_date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_budget(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

#[tauri::command]
pub fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT b.id,b.category_id,b.period,b.amount_cents,b.start_date,b.created_at,b.updated_at,b.version,b.device_id,b.is_deleted,c.name, \
         COALESCE((SELECT SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
                                    WHEN t.kind='refund' THEN -t.amount_native_cents \
                                    ELSE 0 END) \
                   FROM transactions t \
                   JOIN categories tc ON tc.id=t.category_id \
                   WHERE (tc.id=b.category_id OR tc.parent_id=b.category_id) \
                     AND t.is_deleted=0 \
                     AND substr(t.date,1,7)=substr(b.start_date,1,7)),0) \
         FROM budgets b LEFT JOIN categories c ON c.id=b.category_id \
         WHERE b.is_deleted=0 ORDER BY b.created_at",
    )?;
    let rows = stmt.query_map([], |r| {
        let amount_cents: i64 = r.get(3)?;
        let spent: i64 = r.get(10)?;
        Ok(BudgetProgress {
            budget: Budget {
                id: r.get(0)?,
                category_id: r.get(1)?,
                period: r.get(2)?,
                amount_cents,
                start_date: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
                version: r.get(7)?,
                device_id: r.get(8)?,
                is_deleted: r.get::<_, i64>(9)? != 0,
            },
            category_name: r
                .get::<_, Option<String>>(11)?
                .unwrap_or_else(|| "未分类".into()),
            spent_cents: spent,
            over_budget: spent > amount_cents,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
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

    fn insert_budget(conn: &rusqlite::Connection, id: &str, category_id: &str, amount_cents: i64, start_date: &str) {
        let now = now_iso();
        conn.execute(
            "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'monthly',?3,?4,?5,?6,?7,?8,0)",
            rusqlite::params![id, category_id, amount_cents, start_date, now, now, 1, device_id()],
        ).unwrap();
    }

    fn insert_tx(conn: &rusqlite::Connection, id: &str, kind: &str, amount_cents: i64, category_id: &str, date: &str) {
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
        let mut stmt = conn.prepare(
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
        ).unwrap();
        let rows = stmt.query_map([], |r| {
            let amount_cents: i64 = r.get(2)?;
            let spent: i64 = r.get(3)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                amount_cents,
                spent,
                spent > amount_cents,
            ))
        }).unwrap();
        rows.map(|r| {
            let (id, cat_id, budget_amt, spent, over) = r.unwrap();
            (id, cat_id, budget_amt, spent, over)
        }).collect()
    }

    #[test]
    fn list_budgets_empty_initially() {
        let conn = setup();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn create_budget_and_list() {
        let conn = setup();
        let cat_id = first_expense_category_id(&conn);
        insert_budget(&conn, "budget-1", &cat_id, 50000, "2026-07-01");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn delete_budget_soft_deletes() {
        let conn = setup();
        let cat_id = first_expense_category_id(&conn);
        insert_budget(&conn, "budget-2", &cat_id, 50000, "2026-07-01");
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r.get(0)
            ).unwrap(),
            1,
        );
        conn.execute(
            "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
            rusqlite::params!["budget-2", now_iso(), device_id()],
        ).unwrap();
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT COUNT(*) FROM budgets WHERE is_deleted=0", [], |r| r.get(0)
            ).unwrap(),
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
        assert_eq!(results[0].3, 0);     // spent
        assert!(!results[0].4);           // not over budget
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
}
