use tauri::State;

use crate::db::DbState;
use crate::db::query::query_all;
use crate::error::{AppError, Result};
use crate::models::{CategoryShare, MonthlySummary};

#[tauri::command]
pub fn monthly_summary(db: State<'_, DbState>, year: i64) -> Result<Vec<MonthlySummary>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT substr(date,1,7) AS month, \
         SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END) AS income, \
         SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END) AS expense, \
         SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) AS refund \
         FROM transactions WHERE substr(date,1,4)=?1 AND is_deleted=0 \
         GROUP BY month ORDER BY month",
        rusqlite::params![format!("{year}")],
    )
}

#[tauri::command]
pub fn category_shares(
    db: State<'_, DbState>,
    kind: String,
    month: Option<String>,
) -> Result<Vec<CategoryShare>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let (kinds, sign_expr) = if kind == "expense" {
        (
            "'expense','refund'",
            "CASE WHEN t.kind='expense' THEN t.amount_native_cents \
              WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END",
        )
    } else {
        ("'income'", "t.amount_native_cents")
    };
    let mut sql = format!(
        "SELECT t.category_id, COALESCE(c.name,'未分类'), SUM({sign_expr}) AS net \
         FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind IN ({kinds}) AND t.is_deleted=0"
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(m) = month {
        sql.push_str(" AND substr(t.date,1,7)=?1");
        params_vec.push(Box::new(m));
    }
    sql.push_str(" GROUP BY t.category_id ORDER BY net DESC");
    let params_ref: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    query_all(&conn, &sql, params_ref.as_slice())
}

#[cfg(test)]
mod tests {
    use crate::db::query::query_all;
    use crate::db::{device_id, now_iso};
    use crate::models::{CategoryShare, MonthlySummary};

    fn setup() -> rusqlite::Connection {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn insert_account(conn: &rusqlite::Connection, id: &str) {
        let now = now_iso();
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'cash','CNY',0,?3,?4,?5,?6,0)",
            rusqlite::params![id, "测试账户", now, now, 1, device_id()],
        ).unwrap();
    }

    fn insert_tx(
        conn: &rusqlite::Connection,
        id: &str,
        kind: &str,
        amount: i64,
        category_id: Option<&str>,
        date: &str,
    ) {
        let now = now_iso();
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,'CNY',?3,'acc',NULL,?4,NULL,NULL,?5,?6,?7,?8,?9,0)",
            rusqlite::params![id, kind, amount, category_id, date, now, now, 1, device_id()],
        ).unwrap();
    }

    // ---- monthly_summary tests ----

    #[test]
    fn monthly_summary_empty_when_no_transactions() {
        let conn = setup();
        insert_account(&conn, "acc");
        let rows: Vec<MonthlySummary> = query_all(
            &conn,
            "SELECT substr(date,1,7) AS month, \
             SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
             FROM transactions WHERE substr(date,1,4)='2026' AND is_deleted=0 \
             GROUP BY month ORDER BY month",
            [],
        )
        .unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn monthly_summary_groups_by_month() {
        let conn = setup();
        insert_account(&conn, "acc");
        insert_tx(&conn, "t1", "income", 1000, None, "2026-01-15");
        insert_tx(&conn, "t2", "expense", 500, None, "2026-01-20");
        insert_tx(&conn, "t3", "income", 2000, None, "2026-02-10");
        let rows: Vec<MonthlySummary> = query_all(
            &conn,
            "SELECT substr(date,1,7) AS month, \
             SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
             FROM transactions WHERE substr(date,1,4)='2026' AND is_deleted=0 \
             GROUP BY month ORDER BY month",
            [],
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].month, "2026-01");
        assert_eq!(rows[0].income_cents, 1000);
        assert_eq!(rows[0].expense_cents, 500);
        assert_eq!(rows[1].month, "2026-02");
        assert_eq!(rows[1].income_cents, 2000);
    }

    #[test]
    fn monthly_summary_separates_refund() {
        let conn = setup();
        insert_account(&conn, "acc");
        insert_tx(&conn, "t1", "expense", 1000, None, "2026-03-01");
        insert_tx(&conn, "t2", "refund", 200, None, "2026-03-05");
        let rows: Vec<MonthlySummary> = query_all(
            &conn,
            "SELECT substr(date,1,7) AS month, \
             SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
             FROM transactions WHERE substr(date,1,4)='2026' AND is_deleted=0 \
             GROUP BY month ORDER BY month",
            [],
        )
        .unwrap();
        assert_eq!(rows[0].expense_cents, 1000);
        assert_eq!(rows[0].refund_cents, 200);
    }

    #[test]
    fn monthly_summary_filters_by_year() {
        let conn = setup();
        insert_account(&conn, "acc");
        insert_tx(&conn, "t1", "income", 1000, None, "2025-12-31");
        insert_tx(&conn, "t2", "income", 2000, None, "2026-01-01");
        let rows_2025: Vec<MonthlySummary> = query_all(
            &conn,
            "SELECT substr(date,1,7) AS month, \
             SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
             FROM transactions WHERE substr(date,1,4)='2025' AND is_deleted=0 \
             GROUP BY month ORDER BY month",
            [],
        )
        .unwrap();
        assert_eq!(rows_2025.len(), 1);
        assert_eq!(rows_2025[0].income_cents, 1000);
        let rows_2026: Vec<MonthlySummary> = query_all(
            &conn,
            "SELECT substr(date,1,7) AS month, \
             SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END), \
             SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
             FROM transactions WHERE substr(date,1,4)='2026' AND is_deleted=0 \
             GROUP BY month ORDER BY month",
            [],
        )
        .unwrap();
        assert_eq!(rows_2026.len(), 1);
        assert_eq!(rows_2026[0].income_cents, 2000);
    }

    fn first_expense_cat_id(conn: &rusqlite::Connection) -> String {
        conn.query_row(
            "SELECT id FROM categories WHERE kind='expense' AND parent_id IS NULL ORDER BY created_at LIMIT 1",
            [],
            |r| r.get(0),
        ).unwrap()
    }

    fn first_income_cat_id(conn: &rusqlite::Connection) -> String {
        conn.query_row(
            "SELECT id FROM categories WHERE kind='income' AND parent_id IS NULL ORDER BY created_at LIMIT 1",
            [],
            |r| r.get(0),
        ).unwrap()
    }

    // ---- category_shares tests ----

    #[test]
    fn category_shares_expense_includes_refund_as_negative() {
        let conn = setup();
        insert_account(&conn, "acc");
        let cat_id = first_expense_cat_id(&conn);
        insert_tx(&conn, "t1", "expense", 1000, Some(&cat_id), "2026-01-15");
        insert_tx(&conn, "t2", "refund", 200, Some(&cat_id), "2026-01-20");
        let rows: Vec<CategoryShare> = query_all(
            &conn,
            "SELECT t.category_id, COALESCE(c.name,'未分类'), \
             SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
                      WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END) \
             FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
             WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 \
             GROUP BY t.category_id ORDER BY 3 DESC",
            [],
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].amount_cents, 800);
    }

    #[test]
    fn category_shares_income_only_income() {
        let conn = setup();
        insert_account(&conn, "acc");
        let cat_inc = first_income_cat_id(&conn);
        let cat_exp = first_expense_cat_id(&conn);
        insert_tx(&conn, "t1", "income", 5000, Some(&cat_inc), "2026-01-15");
        insert_tx(&conn, "t2", "expense", 1000, Some(&cat_exp), "2026-01-16");
        let rows: Vec<CategoryShare> = query_all(
            &conn,
            "SELECT t.category_id, COALESCE(c.name,'未分类'), SUM(t.amount_native_cents) \
             FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
             WHERE t.kind IN ('income') AND t.is_deleted=0 \
             GROUP BY t.category_id ORDER BY 3 DESC",
            [],
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].amount_cents, 5000);
    }

    #[test]
    fn category_shares_filters_by_month() {
        let conn = setup();
        insert_account(&conn, "acc");
        let cat_id = first_expense_cat_id(&conn);
        insert_tx(&conn, "t1", "expense", 1000, Some(&cat_id), "2026-01-15");
        insert_tx(&conn, "t2", "expense", 2000, Some(&cat_id), "2026-02-10");
        let rows_jan: Vec<CategoryShare> = query_all(
            &conn,
            "SELECT t.category_id, COALESCE(c.name,'未分类'), SUM(t.amount_native_cents) \
             FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
             WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 AND substr(t.date,1,7)='2026-01' \
             GROUP BY t.category_id ORDER BY 3 DESC",
            [],
        ).unwrap();
        assert_eq!(rows_jan[0].amount_cents, 1000);
    }

    #[test]
    fn category_shares_unclassified_shows_default_name() {
        let conn = setup();
        insert_account(&conn, "acc");
        insert_tx(&conn, "t1", "expense", 500, None, "2026-01-15");
        let rows: Vec<CategoryShare> = query_all(
            &conn,
            "SELECT t.category_id, COALESCE(c.name,'未分类'), \
             SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
                      WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END) \
             FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
             WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 \
             GROUP BY t.category_id ORDER BY 3 DESC",
            [],
        )
        .unwrap();
        assert_eq!(rows[0].category_name, "未分类");
    }
}
