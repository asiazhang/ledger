use rusqlite::{Connection, params};
use tauri::{Manager, State};

use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    Account, AccountBalance, AccountInput, Budget, BudgetInput, BudgetPeriod, BudgetProgress, Category,
    CategoryInput, CategoryShare, Currency, ExchangeRate, ExchangeRateInput, Holding, ImportRequest,
    ImportedRow, MarketPrice, MarketPriceInput, MonthlySummary, Transaction, TransactionInput,
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
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
         FROM accounts WHERE is_deleted=0 ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Account {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            currency_code: r.get(3)?,
            initial_balance_cents: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            version: r.get(7)?,
            device_id: r.get(8)?,
            is_deleted: r.get::<_, i64>(9)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_account(db: State<'_, DbState>, input: AccountInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        params![
            id,
            input.name,
            input.kind,
            input.currency_code,
            input.initial_balance_cents.unwrap_or(0),
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_account(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 计算账户当前余额 = 初始余额 + 收入 - 支出（转账从转出账户减，加到转入账户）。
fn account_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    let initial: i64 = conn.query_row(
        "SELECT initial_balance_cents FROM accounts WHERE id=?1",
        params![account_id],
        |r| r.get(0),
    )?;
    let income: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='income' AND is_deleted=0",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    let expense: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='expense' AND is_deleted=0",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_in: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE to_account_id=?1 AND kind='transfer' AND is_deleted=0",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_out: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='transfer' AND is_deleted=0",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    // 退款退回原账户（refund 继承原交易 account_id），计入账户余额。
    let refund: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='refund' AND is_deleted=0",
            params![account_id],
            |r| r.get(0),
        )
        .ok();
    Ok(
        initial + income.unwrap_or(0) - expense.unwrap_or(0) + transfer_in.unwrap_or(0)
            - transfer_out.unwrap_or(0)
            + refund.unwrap_or(0),
    )
}

#[tauri::command]
pub fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
         FROM accounts WHERE is_deleted=0 ORDER BY created_at",
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
                updated_at: r.get(6)?,
                version: r.get(7)?,
                device_id: r.get(8)?,
                is_deleted: r.get::<_, i64>(9)? != 0,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);
    accounts
        .into_iter()
        .map(|a| {
            Ok(AccountBalance {
                balance_cents: account_balance(&conn, &a.id)?,
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
        "SELECT id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted \
         FROM categories WHERE is_deleted=0 ORDER BY kind, created_at",
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
            updated_at: r.get(7)?,
            version: r.get(8)?,
            device_id: r.get(9)?,
            is_deleted: r.get::<_, i64>(10)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_category(db: State<'_, DbState>, input: CategoryInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO categories (id,name,kind,parent_id,icon,color,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,NULL,NULL,?5,?6,?7,?8,0)",
        params![id, input.name, input.kind, input.parent_id, now, now, 1, device_id()],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_category(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE categories SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 交易
// ---------------------------------------------------------------------------

/// 查询账户本位币代码。
pub(crate) fn account_currency_code(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        params![account_id],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

/// 查询货币对的当前汇率。exchange_rates 每货币对仅保留一行最新，无需日期参数。
/// 查不到正向 (base→quote) 时，兜底查反向 (quote→base) 并取倒数。
pub(crate) fn exchange_rate(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
) -> Result<f64> {
    if base_code == quote_code {
        return Ok(1.0);
    }
    if let Some(rate) = conn
        .query_row(
            "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            params![base_code, quote_code],
            |r| r.get(0),
        )
        .ok()
    {
        return Ok(rate);
    }
    // 反向兜底：1 / (quote→base 的 rate)
    if let Some(rev) = conn
        .query_row(
            "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            params![quote_code, base_code],
            |r| r.get::<_, f64>(0),
        )
        .ok()
    {
        if rev <= 0.0 {
            return Err(AppError::Invalid(format!(
                "反向汇率 {quote_code}->{base_code} 非正: {rev}"
            )));
        }
        return Ok(1.0 / rev);
    }
    Err(AppError::Invalid(format!(
        "未找到 {base_code} -> {quote_code} 的汇率（正反向均无）"
    )))
}

/// 将交易金额折算为账户本位币金额。汇率取当前最新，无需交易日期。
pub(crate) fn convert_to_native(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
    account_id: &str,
) -> Result<i64> {
    let account_currency = account_currency_code(conn, account_id)?;
    if currency_code == account_currency {
        Ok(amount_cents)
    } else {
        let rate = exchange_rate(conn, currency_code, &account_currency)?;
        Ok((amount_cents as f64 * rate).round() as i64)
    }
}

#[tauri::command]
pub fn list_transactions(db: State<'_, DbState>, limit: Option<i64>) -> Result<Vec<Transaction>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let base_sql = "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
         to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted \
         FROM transactions WHERE is_deleted=0 ORDER BY date DESC, created_at DESC";
    let sql = match limit {
        Some(n) => format!("{base_sql} LIMIT {n}"),
        None => String::from(base_sql),
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
            refund_of_transaction_id: r.get(8)?,
            note: r.get(9)?,
            date: r.get(10)?,
            created_at: r.get(11)?,
            updated_at: r.get(12)?,
            version: r.get(13)?,
            device_id: r.get(14)?,
            is_deleted: r.get::<_, i64>(15)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_transaction(db: State<'_, DbState>, input: TransactionInput) -> Result<String> {
    if input.amount_cents <= 0 {
        return Err(AppError::Invalid("金额必须大于 0".into()));
    }
    if input.kind == "transfer" && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;

    // 退款必须从已有支出交易发起：校验原交易存在且 kind='expense'，
    // 并强制继承原交易的分类、账户、币种（忽略前端传入的不一致值）。
    let (category_id, account_id, currency_code, refund_of_id) = if input.kind == "refund" {
        let ref_id = input
            .refund_of_transaction_id
            .ok_or_else(|| AppError::Invalid("退款必须关联原支出交易".into()))?;
        let (cat, acc, cur, okind): (Option<String>, String, String, String) = conn.query_row(
            "SELECT category_id, account_id, currency_code, kind \
             FROM transactions WHERE id=?1 AND is_deleted=0",
            params![ref_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if okind != "expense" {
            return Err(AppError::Invalid("退款只能关联支出交易".into()));
        }
        (cat, acc, cur, Some(ref_id))
    } else {
        (
            input.category_id,
            input.account_id,
            input.currency_code.clone(),
            None,
        )
    };

    // 按当前最新汇率将交易币种金额折算到账户本位币。
    let native = convert_to_native(&conn, input.amount_cents, &currency_code, &account_id)?;
    let to_account_id = if input.kind == "transfer" {
        input.to_account_id
    } else {
        None
    };
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,0)",
        params![
            id,
            input.kind,
            input.amount_cents,
            currency_code,
            native,
            account_id,
            to_account_id,
            category_id,
            refund_of_id,
            input.note,
            input.date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE transactions SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 汇率
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_exchange_rates(db: State<'_, DbState>) -> Result<Vec<ExchangeRate>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id \
         FROM exchange_rates ORDER BY base_code, quote_code",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ExchangeRate {
            id: r.get(0)?,
            base_code: r.get(1)?,
            quote_code: r.get(2)?,
            rate: r.get(3)?,
            priced_at: r.get(4)?,
            source: r.get(5)?,
            updated_at: r.get(6)?,
            version: r.get(7)?,
            device_id: r.get(8)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_exchange_rate(db: State<'_, DbState>, input: ExchangeRateInput) -> Result<String> {
    if input.rate <= 0.0 {
        return Err(AppError::Invalid("汇率必须大于 0".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    // 每个货币对只保留一行最新汇率；已存在则更新，否则插入。
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            params![input.base_code, input.quote_code],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or(id);
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9) \
         ON CONFLICT(base_code, quote_code) DO UPDATE SET \
         rate=excluded.rate, priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1, device_id=excluded.device_id",
        params![
            id,
            input.base_code,
            input.quote_code,
            input.rate,
            input.priced_at,
            input.source,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// 市场价格
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_market_prices(db: State<'_, DbState>) -> Result<Vec<MarketPrice>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id \
         FROM market_prices ORDER BY instrument_id, priced_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MarketPrice {
            id: r.get(0)?,
            instrument_id: r.get(1)?,
            price_cents: r.get(2)?,
            currency_code: r.get(3)?,
            priced_at: r.get(4)?,
            source: r.get(5)?,
            created_at: r.get(6)?,
            updated_at: r.get(7)?,
            version: r.get(8)?,
            device_id: r.get(9)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_market_price(db: State<'_, DbState>, input: MarketPriceInput) -> Result<String> {
    if input.price_cents <= 0 {
        return Err(AppError::Invalid("价格必须大于 0".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    // 每个 instrument 只保留一行最新价格；已存在则更新，否则插入。
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM market_prices WHERE instrument_id=?1",
            params![input.instrument_id],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or(id);
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) \
         ON CONFLICT(instrument_id) DO UPDATE SET \
         price_cents=excluded.price_cents, currency_code=excluded.currency_code, \
         priced_at=excluded.priced_at, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1, device_id=excluded.device_id",
        params![
            id,
            input.instrument_id,
            input.price_cents,
            input.currency_code,
            input.priced_at,
            input.source,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// 持仓
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_holdings(db: State<'_, DbState>) -> Result<Vec<Holding>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,account_id,instrument_id,quantity,cost_basis_cents,cost_currency_code, \
         latest_price_cents,latest_price_currency_code,market_value_cents,unrealized_pnl_cents,updated_at \
         FROM v_holdings ORDER BY account_id, instrument_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Holding {
            id: r.get(0)?,
            account_id: r.get(1)?,
            instrument_id: r.get(2)?,
            quantity: r.get(3)?,
            cost_basis_cents: r.get(4)?,
            cost_currency_code: r.get(5)?,
            latest_price_cents: r.get(6)?,
            latest_price_currency_code: r.get(7)?,
            market_value_cents: r.get(8)?,
            unrealized_pnl_cents: r.get(9)?,
            updated_at: r.get(10)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

// ---------------------------------------------------------------------------
// 预算
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn
        .prepare("SELECT id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted \
         FROM budgets WHERE is_deleted=0 ORDER BY created_at")?;
    let rows = stmt.query_map([], |r| {
        Ok(Budget {
            id: r.get(0)?,
            category_id: r.get(1)?,
            period: r.get(2)?,
            amount_cents: r.get(3)?,
            start_date: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            version: r.get(7)?,
            device_id: r.get(8)?,
            is_deleted: r.get::<_, i64>(9)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
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
        params![
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
        params![id, now_iso(), device_id()],
    )?;
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
         SUM(CASE WHEN kind='expense' THEN amount_native_cents ELSE 0 END) AS expense, \
         SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) AS refund \
         FROM transactions WHERE substr(date,1,4)=?1 AND is_deleted=0 \
         GROUP BY month ORDER BY month",
    )?;
    let rows = stmt.query_map(params![format!("{year}")], |r| {
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
    // 支出饼图按净支出（expense - refund）聚合；收入饼图仅聚合 income。
    // 退款复用原支出交易的 category_id，因此与 expense 同分类聚合即可冲减。
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
    let mut conn = crate::db::open_connection(db_path)?;
    crate::db::init_db(&mut conn)?;
    Ok(DbState {
        conn: std::sync::Mutex::new(conn),
    })
}
