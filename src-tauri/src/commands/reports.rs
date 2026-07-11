use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::{CategoryShare, MonthlySummary};

#[tauri::command]
pub fn monthly_summary(db: State<'_, DbState>, year: i64) -> Result<Vec<MonthlySummary>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT substr(date,1,7) AS month, \
         SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END) AS income, \
         SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END) AS expense, \
         SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) AS refund \
         FROM transactions WHERE substr(date,1,4)=?1 AND is_deleted=0 \
         GROUP BY month ORDER BY month",
    )?;
    let rows = stmt.query_map(rusqlite::params![format!("{year}")], |r| {
        Ok(MonthlySummary {
            month: r.get::<_, String>(0)?,
            income_cents: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
            expense_cents: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
            refund_cents: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
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
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |r| {
        Ok(CategoryShare {
            category_id: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
            category_name: r.get(1)?,
            amount_cents: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}
