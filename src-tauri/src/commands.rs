use rusqlite::{Connection, params};
use tauri::{Manager, State};

use crate::db::now_iso;
use crate::error::{AppError, Result};
use crate::models::{
    Account, AccountBalance, AccountInput, Budget, BudgetInput, BudgetProgress, Category,
    CategoryInput, CategoryShare, Currency, ImportRequest, ImportedRow, MonthlySummary,
    Transaction, TransactionInput,
};

// ---------------------------------------------------------------------------
// 币种
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt =
        conn.prepare("SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code")?;
    let rows = stmt.query_map([], |r| {
        Ok(Currency {
            code: r.get(0)?,
            name: r.get(1)?,
            symbol: r.get(2)?,
            decimal_places: r.get(3)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// 账户
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(db: State<'_, DbState>) -> Result<Vec<Account>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at FROM accounts ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Account {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            currency_code: r.get(3)?,
            initial_balance_cents: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_account(db: State<'_, DbState>, input: AccountInput) -> Result<i64> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "INSERT INTO accounts (name,type,currency_code,initial_balance_cents,created_at) \
         VALUES (?1,?2,?3,?4,?5)",
        params![
            input.name,
            input.kind,
            input.currency_code,
            input.initial_balance_cents.unwrap_or(0),
            now_iso()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn delete_account(db: State<'_, DbState>, id: i64) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute("DELETE FROM accounts WHERE id=?1", params![id])?;
    Ok(())
}

/// 计算账户当前余额 = 初始余额 + 收入 - 支出（转账从转出账户减，加到转入账户）。
fn account_balance(conn: &Connection, account_id: i64) -> Result<i64> {
    let initial: i64 = conn.query_row(
        "SELECT initial_balance_cents FROM accounts WHERE id=?1",
        params![account_id],
        |r| r.get(0),
    )?;
    let income: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='income'",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    let expense: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='expense'",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_in: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE to_account_id=?1 AND kind='transfer'",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_out: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='transfer'",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    Ok(
        initial + income.unwrap_or(0) - expense.unwrap_or(0) + transfer_in.unwrap_or(0)
            - transfer_out.unwrap_or(0),
    )
}

#[tauri::command]
pub fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at FROM accounts ORDER BY id",
    )?;
    let accounts: Vec<Account> = stmt
        .query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                currency_code: r.get(3)?,
                initial_balance_cents: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);
    accounts
        .into_iter()
        .map(|a| {
            Ok(AccountBalance {
                balance_cents: account_balance(&conn, a.id)?,
                account: a,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 分类
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_categories(db: State<'_, DbState>) -> Result<Vec<Category>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,name,kind,parent_id,icon,color,created_at FROM categories ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Category {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            parent_id: r.get(3)?,
            icon: r.get(4)?,
            color: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_category(db: State<'_, DbState>, input: CategoryInput) -> Result<i64> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "INSERT INTO categories (name,kind,parent_id,created_at) VALUES (?1,?2,?3,?4)",
        params![input.name, input.kind, input.parent_id, now_iso()],
    )?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn delete_category(db: State<'_, DbState>, id: i64) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute("DELETE FROM categories WHERE id=?1", params![id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 交易
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_transactions(db: State<'_, DbState>, limit: Option<i64>) -> Result<Vec<Transaction>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let sql = match limit {
        Some(n) => format!(
            "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
             to_account_id,category_id,note,date,created_at FROM transactions \
             ORDER BY date DESC, id DESC LIMIT {n}"
        ),
        None => String::from(
            "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
             to_account_id,category_id,note,date,created_at FROM transactions \
             ORDER BY date DESC, id DESC",
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(Transaction {
            id: r.get(0)?,
            kind: r.get(1)?,
            amount_cents: r.get(2)?,
            currency_code: r.get(3)?,
            amount_native_cents: r.get(4)?,
            account_id: r.get(5)?,
            to_account_id: r.get(6)?,
            category_id: r.get(7)?,
            note: r.get(8)?,
            date: r.get(9)?,
            created_at: r.get(10)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_transaction(db: State<'_, DbState>, input: TransactionInput) -> Result<i64> {
    if input.amount_cents <= 0 {
        return Err(AppError::Invalid("金额必须大于 0".into()));
    }
    if input.kind == "transfer" && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }
    // MVP：amount_native 直接等于 amount_cents（暂按 1:1，多币种换算留待后续）。
    let native = input.amount_cents;
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "INSERT INTO transactions \
         (kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,note,date,created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            input.kind,
            input.amount_cents,
            input.currency_code,
            native,
            input.account_id,
            input.to_account_id,
            input.category_id,
            input.note,
            input.date,
            now_iso()
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: i64) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute("DELETE FROM transactions WHERE id=?1", params![id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 预算
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn
        .prepare("SELECT id,category_id,period,amount_cents,start_date FROM budgets ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Budget {
            id: r.get(0)?,
            category_id: r.get(1)?,
            period: r.get(2)?,
            amount_cents: r.get(3)?,
            start_date: r.get(4)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_budget(db: State<'_, DbState>, input: BudgetInput) -> Result<i64> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "INSERT INTO budgets (category_id,period,amount_cents,start_date) \
         VALUES (?1,?2,?3,?4)",
        params![
            input.category_id,
            input.period.unwrap_or_else(|| "monthly".into()),
            input.amount_cents,
            input.start_date
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn delete_budget(db: State<'_, DbState>, id: i64) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute("DELETE FROM budgets WHERE id=?1", params![id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 报表
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn monthly_summary(db: State<'_, DbState>, year: i64) -> Result<Vec<MonthlySummary>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT substr(date,1,7) AS month, \
         SUM(CASE WHEN kind='income' THEN amount_native_cents ELSE 0 END) AS income, \
         SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END) AS expense \
         FROM transactions WHERE substr(date,1,4)=?1 \
         GROUP BY month ORDER BY month",
    )?;
    let rows = stmt.query_map(params![format!("{year}")], |r| {
        Ok(MonthlySummary {
            month: r.get::<_, String>(0)?,
            income_cents: r.get::<_, Option<i64>>(1)?.unwrap_or(0),
            expense_cents: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
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
    let mut sql = String::from(
        "SELECT t.category_id, COALESCE(c.name,'未分类'), SUM(t.amount_native_cents) \
         FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind=?1",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(kind.clone())];
    if let Some(m) = month {
        sql.push_str(" AND substr(t.date,1,7)=?2");
        params_vec.push(Box::new(m));
    }
    sql.push_str(" GROUP BY t.category_id ORDER BY SUM(t.amount_native_cents) DESC");
    let params_ref: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_ref.as_slice(), |r| {
        Ok(CategoryShare {
            category_id: r.get::<_, Option<i64>>(0)?.unwrap_or(0),
            category_name: r.get(1)?,
            amount_cents: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT b.id,b.category_id,b.period,b.amount_cents,b.start_date,c.name, \
         COALESCE((SELECT SUM(amount_native_cents) FROM transactions t \
                   WHERE t.category_id=b.category_id AND t.kind='expense' \
                   AND substr(t.date,1,7)=substr(b.start_date,1,7)),0) \
         FROM budgets b LEFT JOIN categories c ON c.id=b.category_id ORDER BY b.id",
    )?;
    let rows = stmt.query_map([], |r| {
        let amount_cents: i64 = r.get(3)?;
        let spent: i64 = r.get(6)?;
        Ok(BudgetProgress {
            budget: Budget {
                id: r.get(0)?,
                category_id: r.get(1)?,
                period: r.get(2)?,
                amount_cents,
                start_date: r.get(4)?,
            },
            category_name: r
                .get::<_, Option<String>>(5)?
                .unwrap_or_else(|| "未分类".into()),
            spent_cents: spent,
            over_budget: spent > amount_cents,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// 导入解析（CSV / Excel）
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn preview_import(db: State<'_, DbState>, req: ImportRequest) -> Result<Vec<ImportedRow>> {
    // 仅做解析预览，不写库；db 参数保留以便未来按账户币种做换算。
    let _ = &db;
    let path = req.path.as_str();
    if path.to_lowercase().ends_with(".csv") {
        parse_csv(path)
    } else if let Some(ext) = path.rsplit('.').next()
        && matches!(ext.to_lowercase().as_str(), "xlsx" | "xls")
    {
        parse_excel(path)
    } else {
        Err(AppError::Invalid("仅支持 .csv / .xlsx / .xls 文件".into()))
    }
}

fn parse_csv(path: &str) -> Result<Vec<ImportedRow>> {
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_path(path)?;
    let headers: std::collections::HashMap<String, usize> = rdr
        .headers()?
        .iter()
        .enumerate()
        .map(|(i, h)| (h.trim().to_lowercase(), i))
        .collect();
    let date_idx = headers.get("date").or_else(|| headers.get("日期"));
    let amount_idx = headers.get("amount").or_else(|| headers.get("金额"));
    let note_idx = headers
        .get("note")
        .or_else(|| headers.get("备注"))
        .or_else(|| headers.get("描述"));
    let cat_idx = headers.get("category").or_else(|| headers.get("分类"));
    let mut out = Vec::new();
    for record in rdr.records() {
        let record = record?;
        let date = date_idx
            .and_then(|i| record.get(*i))
            .unwrap_or("")
            .trim()
            .to_string();
        let amount_raw = amount_idx.and_then(|i| record.get(*i)).unwrap_or("").trim();
        let amount_cents = parse_amount_cents(amount_raw)?;
        let note = note_idx
            .and_then(|i| record.get(*i))
            .unwrap_or("")
            .trim()
            .to_string();
        let category_name = cat_idx
            .and_then(|i| record.get(*i))
            .map(|s| s.trim().to_string());
        if date.is_empty() {
            continue;
        }
        out.push(ImportedRow {
            date,
            amount_cents,
            note,
            category_name,
        });
    }
    Ok(out)
}

fn parse_excel(path: &str) -> Result<Vec<ImportedRow>> {
    use calamine::{Reader, open_workbook_auto};
    let mut workbook =
        open_workbook_auto(path).map_err(|e| AppError::Parse(format!("打开 Excel 失败: {e}")))?;
    let sheet = workbook
        .worksheets()
        .first()
        .map(|(name, _)| name.clone())
        .ok_or_else(|| AppError::Parse("Excel 无工作表".into()))?;
    let range = workbook
        .worksheet_range(&sheet)
        .map_err(|e| AppError::Parse(format!("读取工作表失败: {e}")))?;
    let mut iter = range.rows();
    let header = iter
        .next()
        .ok_or_else(|| AppError::Parse("Excel 无表头".into()))?;
    let header_map: std::collections::HashMap<String, usize> = header
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let s = c.to_string().trim().to_lowercase();
            if s.is_empty() { None } else { Some((s, i)) }
        })
        .collect();
    let date_idx = header_map.get("date").or_else(|| header_map.get("日期"));
    let amount_idx = header_map.get("amount").or_else(|| header_map.get("金额"));
    let note_idx = header_map
        .get("note")
        .or_else(|| header_map.get("备注"))
        .or_else(|| header_map.get("描述"));
    let cat_idx = header_map
        .get("category")
        .or_else(|| header_map.get("分类"));
    let mut out = Vec::new();
    for row in iter {
        let cell = |i: &usize| -> String {
            row.get(*i)
                .map(|c| c.to_string())
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        let date = date_idx.map(cell).unwrap_or_default();
        let amount_raw = amount_idx.map(cell).unwrap_or_default();
        let amount_cents = parse_amount_cents(amount_raw.as_str())?;
        let note = note_idx.map(cell).unwrap_or_default();
        let category_name = cat_idx.map(cell);
        if date.is_empty() {
            continue;
        }
        out.push(ImportedRow {
            date,
            amount_cents,
            note,
            category_name,
        });
    }
    Ok(out)
}

/// 将字符串金额转为整数分。支持 "12.34"、"1,234.56"、负数。
fn parse_amount_cents(raw: &str) -> Result<i64> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',')
        .collect();
    if cleaned.is_empty() {
        return Ok(0);
    }
    let parsed: f64 = cleaned
        .parse()
        .map_err(|e| AppError::Parse(format!("无法解析金额 '{raw}': {e}")))?;
    Ok((parsed * 100.0).round() as i64)
}

// ---------------------------------------------------------------------------
// 应用状态
// ---------------------------------------------------------------------------

pub struct DbState {
    pub conn: std::sync::Mutex<Connection>,
}

pub fn open_db(app: &tauri::AppHandle) -> Result<DbState> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Io(e.to_string()))?;
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("ledger.db");
    let mut conn = Connection::open(db_path)?;
    crate::db::init_db(&mut conn)?;
    Ok(DbState {
        conn: std::sync::Mutex::new(conn),
    })
}
