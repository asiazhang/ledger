use rusqlite::{Connection, params};
use tauri::{Manager, State};

use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    Account, AccountBalance, AccountInput, AccountType, Budget, BudgetInput, BudgetPeriod, BudgetProgress,
    Category, CategoryInput, CategoryShare, Currency, ExchangeRate, ExchangeRateInput, Holding,
    ImportRequest, ImportedRow, Instrument, InstrumentInput, MarketPrice, MarketPriceInput,
    MonthlySummary, Transaction, TransactionInput,
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
    if let Ok(rate) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        params![base_code, quote_code],
        |r| r.get(0),
    ) {
        return Ok(rate);
    }
    // 反向兜底：1 / (quote→base 的 rate)
    if let Ok(rev) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        params![quote_code, base_code],
        |r| r.get::<_, f64>(0),
    ) {
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

/// 买入交易：校验、写入 transactions / security_transactions / security_lots。
fn create_buy_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::Invalid("买入必须指定标的".into()))?
        .clone();
    let quantity = input.quantity.unwrap_or(0.0);
    let price_cents = input.price_cents.unwrap_or(0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    if quantity <= 0.0 {
        return Err(AppError::Invalid("买入数量必须大于 0".into()));
    }
    if price_cents <= 0 {
        return Err(AppError::Invalid("买入单价必须大于 0".into()));
    }
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            params![input.account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    if account_type != AccountType::Investment {
        return Err(AppError::Invalid("买入交易必须使用投资账户".into()));
    }
    let account_currency = account_currency_code(conn, &input.account_id)?;
    let amount_cents = (quantity * price_cents as f64).round() as i64 + fee_cents;

    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,NULL,?8,?9,?10,?11,?12,?13,0)",
        params![
            id,
            input.kind,
            amount_cents,
            account_currency,
            amount_cents,
            input.account_id,
            input.to_account_id,
            input.note,
            input.date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    create_buy_lot(
        conn,
        &id,
        &input.account_id,
        &instrument_id,
        quantity,
        price_cents,
        fee_cents,
        &account_currency,
    )?;
    Ok(id)
}

/// 卖出交易：校验、按 FIFO 匹配同账户同标的未卖出 lot、扣减 remaining_quantity、写入 security_lot_sales。
fn create_sell_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    let instrument_id = input
        .instrument_id
        .as_ref()
        .ok_or_else(|| AppError::Invalid("卖出必须指定标的".into()))?
        .clone();
    let quantity = input.quantity.unwrap_or(0.0);
    let price_cents = input.price_cents.unwrap_or(0);
    let fee_cents = input.fee_cents.unwrap_or(0);
    if quantity <= 0.0 {
        return Err(AppError::Invalid("卖出数量必须大于 0".into()));
    }
    if price_cents <= 0 {
        return Err(AppError::Invalid("卖出单价必须大于 0".into()));
    }
    let account_type: AccountType = conn
        .query_row(
            "SELECT type FROM accounts WHERE id=?1",
            params![input.account_id],
            |r| r.get::<_, String>(0),
        )?
        .parse()?;
    if account_type != AccountType::Investment {
        return Err(AppError::Invalid("卖出交易必须使用投资账户".into()));
    }
    let account_currency = account_currency_code(conn, &input.account_id)?;
    let gross_proceeds = (quantity * price_cents as f64).round() as i64;
    if fee_cents > gross_proceeds {
        return Err(AppError::Invalid("卖出手续费不能超过卖出收入".into()));
    }
    let amount_cents = gross_proceeds - fee_cents;

    // 先按 FIFO 读取可用 lot 并检查是否足够，避免部分扣减后才发现超卖。
    let mut stmt = conn.prepare(
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY created_at ASC, id ASC",
    )?;
    let lots = stmt
        .query_map(params![input.account_id, instrument_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<(String, f64, i64, String)>, _>>()?;
    drop(stmt);

    let total_available: f64 = lots.iter().map(|(_, rem, _, _)| rem).sum();
    if total_available < quantity {
        return Err(AppError::Invalid(format!(
            "可卖出数量不足，当前持有 {}，尝试卖出 {}",
            total_available, quantity
        )));
    }

    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,NULL,NULL,?8,?9,?10,?11,?12,?13,0)",
        params![
            id,
            input.kind,
            amount_cents,
            account_currency,
            amount_cents,
            input.account_id,
            input.to_account_id,
            input.note,
            input.date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'sell',?3,?4,?5)",
        params![id, instrument_id, quantity, price_cents, fee_cents],
    )?;

    // 按 FIFO 实际扣减 lot；手续费按卖出数量比例 floor 分配给前 n-1 笔，最后一笔拿剩余。
    let mut remaining_to_sell = quantity;
    let mut matched_lots: Vec<(String, f64, i64, String)> = Vec::new();
    for (lot_id, rem, cost, ccy) in lots {
        if remaining_to_sell <= 0.0 {
            break;
        }
        let matched = rem.min(remaining_to_sell);
        matched_lots.push((lot_id, matched, cost, ccy));
        remaining_to_sell -= matched;
    }

    let match_count = matched_lots.len();
    let mut allocated_fee_total = 0i64;
    for (i, (lot_id, matched_qty, cost_per_unit, ccy)) in matched_lots.iter().enumerate() {
        let lot_proceeds = (matched_qty * price_cents as f64).round() as i64;
        let lot_cost = (matched_qty * *cost_per_unit as f64).round() as i64;
        let allocated_fee = if i == match_count - 1 {
            fee_cents - allocated_fee_total
        } else {
            let fee = (fee_cents as f64 * matched_qty / quantity).floor() as i64;
            allocated_fee_total += fee;
            fee
        };
        let realized_pnl = lot_proceeds - lot_cost - allocated_fee;
        let sale_id = new_uuid();
        conn.execute(
            "INSERT INTO security_lot_sales (id,sell_transaction_id,lot_id,quantity,cost_per_unit_cents,realized_pnl_cents,currency_code,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![sale_id, id, lot_id, matched_qty, cost_per_unit, realized_pnl, ccy, now],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            params![matched_qty, now, device_id(), lot_id],
        )?;
    }

    Ok(id)
}

#[tauri::command]
pub fn create_transaction(db: State<'_, DbState>, input: TransactionInput) -> Result<String> {
    if input.kind == "transfer" && input.to_account_id.is_none() {
        return Err(AppError::Invalid("转账必须指定目标账户".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;

    if input.kind == "buy" {
        return create_buy_transaction(&conn, input);
    }

    if input.kind == "sell" {
        return create_sell_transaction(&conn, input);
    }

    if input.amount_cents <= 0 {
        return Err(AppError::Invalid("金额必须大于 0".into()));
    }

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
            input.currency_code,
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

/// 买入交易创建对应的 lot。必须在同一个事务中调用，参数已校验。
#[allow(clippy::too_many_arguments)]
fn create_buy_lot(
    conn: &Connection,
    transaction_id: &str,
    account_id: &str,
    instrument_id: &str,
    quantity: f64,
    price_cents: i64,
    fee_cents: i64,
    currency_code: &str,
) -> Result<()> {
    let lot_id = new_uuid();
    let now = now_iso();
    // 单位成本含手续费摊薄：总成本 / 数量，四舍五入到分。
    let total_cost_cents = (quantity * price_cents as f64).round() as i64 + fee_cents;
    let cost_per_unit = if quantity > 0.0 {
        (total_cost_cents as f64 / quantity).round() as i64
    } else {
        0
    };
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',?3,?4,?5)",
        params![transaction_id, instrument_id, quantity, price_cents, fee_cents],
    )?;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        params![
            lot_id,
            account_id,
            instrument_id,
            transaction_id,
            quantity,
            cost_per_unit,
            currency_code,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(())
}

#[tauri::command]
pub fn delete_transaction(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;

    // 买入交易删除时需同步清理未卖出的 lot 与 security_transaction，避免持仓视图仍显示已删交易。
    let is_buy: Option<bool> = conn
        .query_row(
            "SELECT kind='buy' FROM transactions WHERE id=?1 AND is_deleted=0",
            params![id],
            |r| r.get::<_, i64>(0).map(|v| v != 0),
        )
        .ok();
    if is_buy == Some(true) {
        let sold: i64 = conn.query_row(
            "SELECT COUNT(*) FROM security_lots \
             WHERE buy_transaction_id=(SELECT transaction_id FROM security_transactions WHERE transaction_id=?1) \
             AND remaining_quantity < initial_quantity",
            params![id],
            |r| r.get(0),
        )?;
        if sold > 0 {
            return Err(AppError::Invalid(
                "该买入交易已有部分卖出，无法删除".into(),
            ));
        }
        conn.execute(
            "DELETE FROM security_lots WHERE buy_transaction_id=(SELECT transaction_id FROM security_transactions WHERE transaction_id=?1)",
            params![id],
        )?;
        conn.execute(
            "DELETE FROM security_transactions WHERE transaction_id=?1",
            params![id],
        )?;
    }

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
// 金融工具
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_instruments(db: State<'_, DbState>) -> Result<Vec<Instrument>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id \
         FROM instruments ORDER BY symbol",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Instrument {
            id: r.get(0)?,
            symbol: r.get(1)?,
            kind: r.get(2)?,
            name: r.get(3)?,
            currency_code: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            version: r.get(7)?,
            device_id: r.get(8)?,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_instrument(db: State<'_, DbState>, input: InstrumentInput) -> Result<String> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::Invalid("标的代码不能为空".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    // 同 (symbol, type) 已存在则返回已有 ID，避免重复创建。
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            params![input.symbol, input.kind],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = existing_id {
        return Ok(id);
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            id,
            input.symbol,
            input.kind,
            input.name,
            input.currency_code,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    /// create_buy_transaction 写入交易、security_transaction 和未卖出的 lot。
    #[test]
    fn buy_transaction_creates_lot() {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();

        let account_id = "acc-test-buy";
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        let instrument_id = "inst-test-nvda";
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'NVDA','stock','NVIDIA','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();

        let input = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(10.0),
            price_cents: Some(10000),
            fee_cents: Some(500),
        };
        let txn_id = create_buy_transaction(&conn, input).unwrap();

        // 交易总金额为 10 * 10000 + 500 = 100500 分
        let (kind, amount_cents, currency_code): (String, i64, String) = conn
            .query_row(
                "SELECT kind, amount_cents, currency_code FROM transactions WHERE id=?1",
                params![txn_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(kind, "buy");
        assert_eq!(amount_cents, 100500);
        assert_eq!(currency_code, "USD");

        let (action, quantity, price_cents, fee_cents): (String, f64, i64, i64) = conn
            .query_row(
                "SELECT action, quantity, price_cents, fee_cents FROM security_transactions WHERE transaction_id=?1",
                params![txn_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(action, "buy");
        assert!((quantity - 10.0).abs() < 0.0001);
        assert_eq!(price_cents, 10000);
        assert_eq!(fee_cents, 500);

        let (remaining_quantity, cost_per_unit): (f64, i64) = conn
            .query_row(
                "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
                params![txn_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((remaining_quantity - 10.0).abs() < 0.0001);
        // 单位成本 = (10 * 10000 + 500) / 10 = 10050 分
        assert_eq!(cost_per_unit, 10050);

        // 持仓视图能读到该 lot
        let (holding_quantity, cost_basis): (f64, i64) = conn
            .query_row(
                "SELECT quantity, cost_basis_cents FROM v_holdings WHERE id=?1",
                params![format!("{account_id}-{instrument_id}-USD")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((holding_quantity - 10.0).abs() < 0.0001);
        assert_eq!(cost_basis, 100500);
    }

    /// 非投资账户不能录入买入交易。
    #[test]
    fn buy_transaction_requires_investment_account() {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();

        let account_id = "acc-test-cash";
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'现金','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        let instrument_id = "inst-test-cny";
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'600519','stock','茅台','CNY','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();

        let input = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "CNY".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(1.0),
            price_cents: Some(10000),
            fee_cents: None,
        };
        assert!(create_buy_transaction(&conn, input).is_err());
    }

    /// 卖出交易按 FIFO 匹配多 lot，并计算扣除手续费后的已实现盈亏。
    #[test]
    fn sell_transaction_matches_multiple_lots_fifo() {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();

        let account_id = "acc-test-sell";
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        let instrument_id = "inst-test-sell";
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'TSLA','stock','Tesla','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();

        // 买入 lot1：10 股 @ 10000 分，1 月 10 日
        let buy1 = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(10.0),
            price_cents: Some(10000),
            fee_cents: Some(0),
        };
        let lot1_txn = create_buy_transaction(&conn, buy1).unwrap();

        // 买入 lot2：5 股 @ 12000 分，1 月 15 日
        let buy2 = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-15".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(5.0),
            price_cents: Some(12000),
            fee_cents: Some(0),
        };
        let lot2_txn = create_buy_transaction(&conn, buy2).unwrap();

        // 显式设置 lot 创建时间，确保 FIFO 顺序可复现：lot1 早于 lot2
        conn.execute(
            "UPDATE security_lots SET created_at='2026-01-10T00:00:00Z' WHERE buy_transaction_id=?1",
            params![lot1_txn],
        )
        .unwrap();
        conn.execute(
            "UPDATE security_lots SET created_at='2026-01-15T00:00:00Z' WHERE buy_transaction_id=?1",
            params![lot2_txn],
        )
        .unwrap();

        // 卖出 12 股 @ 15000 分，手续费 600 分
        let sell = TransactionInput {
            kind: "sell".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-20".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(12.0),
            price_cents: Some(15000),
            fee_cents: Some(600),
        };
        let sell_txn = create_sell_transaction(&conn, sell).unwrap();

        let (kind, amount_cents): (String, i64) = conn
            .query_row(
                "SELECT kind, amount_cents FROM transactions WHERE id=?1",
                params![sell_txn],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "sell");
        // 卖出净收入 = 12 * 15000 - 600 = 179400 分
        assert_eq!(amount_cents, 179400);

        let (action, qty, price, fee): (String, f64, i64, i64) = conn
            .query_row(
                "SELECT action, quantity, price_cents, fee_cents FROM security_transactions WHERE transaction_id=?1",
                params![sell_txn],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(action, "sell");
        assert!((qty - 12.0).abs() < 0.0001);
        assert_eq!(price, 15000);
        assert_eq!(fee, 600);

        // lot1 卖完，lot2 剩 3 股
        let rem1: f64 = conn
            .query_row(
                "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
                params![lot1_txn],
                |r| r.get(0),
            )
            .unwrap();
        assert!((rem1 - 0.0).abs() < 0.0001);
        let rem2: f64 = conn
            .query_row(
                "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
                params![lot2_txn],
                |r| r.get(0),
            )
            .unwrap();
        assert!((rem2 - 3.0).abs() < 0.0001);

        // 匹配记录：lot1 10 股，lot2 2 股；费用按数量比例分配，剩余费用给最后一笔匹配
        let rows: Vec<(f64, i64, i64, String)> = conn
            .prepare(
                "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
                 FROM security_lot_sales WHERE sell_transaction_id=?1 ORDER BY quantity DESC",
            )
            .unwrap()
            .query_map(params![sell_txn], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(rows.len(), 2);
        // lot1: 10 股，成本 10000，分配费用 round(600 * 10 / 12) = 500，盈亏 150000 - 100000 - 500 = 49500
        assert!((rows[0].0 - 10.0).abs() < 0.0001);
        assert_eq!(rows[0].1, 10000);
        assert_eq!(rows[0].2, 49500);
        assert_eq!(rows[0].3, "USD");
        // lot2: 2 股，成本 12000，分配剩余费用 100，盈亏 30000 - 24000 - 100 = 5900
        assert!((rows[1].0 - 2.0).abs() < 0.0001);
        assert_eq!(rows[1].1, 12000);
        assert_eq!(rows[1].2, 5900);
        assert_eq!(rows[1].3, "USD");
    }

    /// 卖出数量超过可卖数量时，应拒绝并回滚。
    #[test]
    fn sell_transaction_rejects_oversell() {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();

        let account_id = "acc-test-oversell";
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        let instrument_id = "inst-test-oversell";
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'MSFT','stock','Microsoft','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();

        let buy = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(5.0),
            price_cents: Some(10000),
            fee_cents: Some(0),
        };
        create_buy_transaction(&conn, buy).unwrap();

        let sell = TransactionInput {
            kind: "sell".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-20".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(6.0),
            price_cents: Some(12000),
            fee_cents: Some(0),
        };
        assert!(create_sell_transaction(&conn, sell).is_err());
    }

    /// 卖出盈亏已扣除卖出手续费。
    #[test]
    fn sell_transaction_pnl_deducts_fee() {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();

        let account_id = "acc-test-pnl";
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        let instrument_id = "inst-test-pnl";
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'AAPL','stock','Apple','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();

        let buy = TransactionInput {
            kind: "buy".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-10".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(10.0),
            price_cents: Some(10000),
            fee_cents: Some(0),
        };
        let buy_txn = create_buy_transaction(&conn, buy).unwrap();

        let sell = TransactionInput {
            kind: "sell".into(),
            amount_cents: 0,
            currency_code: "USD".into(),
            account_id: account_id.into(),
            to_account_id: None,
            category_id: None,
            refund_of_transaction_id: None,
            note: None,
            date: "2026-01-20".into(),
            instrument_id: Some(instrument_id.into()),
            quantity: Some(5.0),
            price_cents: Some(12000),
            fee_cents: Some(200),
        };
        let sell_txn = create_sell_transaction(&conn, sell).unwrap();

        let rem: f64 = conn
            .query_row(
                "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
                params![buy_txn],
                |r| r.get(0),
            )
            .unwrap();
        assert!((rem - 5.0).abs() < 0.0001);

        let (qty, cost, pnl, ccy): (f64, i64, i64, String) = conn
            .query_row(
                "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
                 FROM security_lot_sales WHERE sell_transaction_id=?1",
                params![sell_txn],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!((qty - 5.0).abs() < 0.0001);
        assert_eq!(cost, 10000);
        // 盈亏 = 5 * 12000 - 5 * 10000 - 200 = 9800
        assert_eq!(pnl, 9800);
        assert_eq!(ccy, "USD");

        let amount_cents: i64 = conn
            .query_row(
                "SELECT amount_cents FROM transactions WHERE id=?1",
                params![sell_txn],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(amount_cents, 5 * 12000 - 200); // 59800
    }
}
