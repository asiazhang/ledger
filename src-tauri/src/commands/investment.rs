use rusqlite::Connection;

use crate::commands::fx::account_currency_code;
use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{
    AccountPnl, AccountType, InstrumentPnl, PnlDetail, PnlFilter, RealizedPnlSummary,
    TransactionInput, YearPnl,
};

/// 买入交易：校验、写入 transactions / security_transactions / security_lots。
pub(crate) fn create_buy_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
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
            rusqlite::params![input.account_id],
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
        rusqlite::params![
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
pub(crate) fn create_sell_transaction(
    conn: &Connection,
    input: TransactionInput,
) -> Result<String> {
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
            rusqlite::params![input.account_id],
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

    let mut stmt = conn.prepare(
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY created_at ASC, id ASC",
    )?;
    let lots = stmt
        .query_map(rusqlite::params![input.account_id, instrument_id], |r| {
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
        rusqlite::params![
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
        rusqlite::params![id, instrument_id, quantity, price_cents, fee_cents],
    )?;

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
            rusqlite::params![sale_id, id, lot_id, matched_qty, cost_per_unit, realized_pnl, ccy, now],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![matched_qty, now, device_id(), lot_id],
        )?;
    }

    Ok(id)
}

/// 买入交易创建对应的 lot。必须在同一个事务中调用，参数已校验。
#[allow(clippy::too_many_arguments)]
pub(crate) fn create_buy_lot(
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
    let total_cost_cents = (quantity * price_cents as f64).round() as i64 + fee_cents;
    let cost_per_unit = if quantity > 0.0 {
        (total_cost_cents as f64 / quantity).round() as i64
    } else {
        0
    };
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',?3,?4,?5)",
        rusqlite::params![transaction_id, instrument_id, quantity, price_cents, fee_cents],
    )?;
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,?6,?7,?8,?8,?9,?10)",
        rusqlite::params![
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

/// 持仓列表（只读查询）
#[tauri::command]
pub fn list_holdings(
    db: tauri::State<'_, crate::db::DbState>,
) -> Result<Vec<crate::models::Holding>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,account_id,instrument_id,quantity,cost_basis_cents,cost_currency_code, \
         latest_price_cents,latest_price_currency_code,market_value_cents,unrealized_pnl_cents,updated_at \
         FROM v_holdings ORDER BY account_id, instrument_id",
        [],
    )
}

/// 已实现盈亏汇总查询（核心逻辑，可被命令和测试共用）。
pub(crate) fn query_realized_pnl_summary(
    conn: &Connection,
    filter: &PnlFilter,
) -> Result<RealizedPnlSummary> {
    let base_from = "FROM security_lot_sales sls \
                     JOIN transactions t ON t.id = sls.sell_transaction_id \
                     JOIN security_transactions st ON st.transaction_id = sls.sell_transaction_id \
                     JOIN instruments i ON i.id = st.instrument_id \
                     JOIN accounts a ON a.id = t.account_id";

    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(acct_id) = &filter.account_id {
        params.push(Box::new(acct_id.clone()));
        conditions.push(format!("t.account_id=?{}", params.len()));
    }
    if let Some(inst_id) = &filter.instrument_id {
        params.push(Box::new(inst_id.clone()));
        conditions.push(format!("st.instrument_id=?{}", params.len()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let total_sql = format!(
        "SELECT COALESCE(SUM(sls.realized_pnl_cents), 0) {}{}",
        base_from, where_clause
    );
    let year_sql = format!(
        "SELECT substr(t.date, 1, 4) AS year, SUM(sls.realized_pnl_cents) \
         {}{} GROUP BY year ORDER BY year",
        base_from, where_clause
    );
    let account_sql = format!(
        "SELECT a.id, a.name, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {}{} GROUP BY a.id ORDER BY a.name",
        base_from, where_clause
    );
    let instrument_sql = format!(
        "SELECT i.id, i.symbol, i.name, COALESCE(SUM(sls.realized_pnl_cents), 0) \
         {}{} GROUP BY i.id ORDER BY i.symbol",
        base_from, where_clause
    );
    let detail_sql = format!(
        "SELECT sls.id, t.date, t.account_id, a.name, i.id, i.symbol, i.name, \
         sls.quantity, sls.cost_per_unit_cents, sls.realized_pnl_cents, sls.currency_code \
         {}{} ORDER BY t.date DESC, sls.created_at DESC",
        base_from, where_clause
    );

    let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

    let total_realized_pnl_cents: i64 = conn
        .query_row(&total_sql, params_ref.as_slice(), |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap_or(None)
        .unwrap_or(0);

    let by_year: Vec<YearPnl> = query_all(conn, &year_sql, params_ref.as_slice())?;
    let by_account: Vec<AccountPnl> = query_all(conn, &account_sql, params_ref.as_slice())?;
    let by_instrument: Vec<InstrumentPnl> =
        query_all(conn, &instrument_sql, params_ref.as_slice())?;
    let details: Vec<PnlDetail> = query_all(conn, &detail_sql, params_ref.as_slice())?;

    Ok(RealizedPnlSummary {
        total_realized_pnl_cents,
        by_year,
        by_account,
        by_instrument,
        details,
    })
}

/// 已实现盈亏汇总：总盈亏、按年度、按账户、按标的、明细。
#[tauri::command]
pub fn realized_pnl_summary(
    db: tauri::State<'_, crate::db::DbState>,
    filter: Option<PnlFilter>,
) -> Result<RealizedPnlSummary> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_realized_pnl_summary(
        &conn,
        &filter.unwrap_or(PnlFilter {
            account_id: None,
            instrument_id: None,
        }),
    )
}

#[tauri::command]
pub fn list_exchange_rates(
    db: tauri::State<'_, crate::db::DbState>,
) -> Result<Vec<crate::models::ExchangeRate>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id \
         FROM exchange_rates ORDER BY base_code, quote_code",
        [],
    )
}

#[tauri::command]
pub fn create_exchange_rate(
    db: tauri::State<'_, crate::db::DbState>,
    input: crate::models::ExchangeRateInput,
) -> Result<String> {
    if input.rate <= 0.0 {
        return Err(AppError::Invalid("汇率必须大于 0".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
            rusqlite::params![input.base_code, input.quote_code],
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
        rusqlite::params![
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

#[tauri::command]
pub fn list_market_prices(
    db: tauri::State<'_, crate::db::DbState>,
) -> Result<Vec<crate::models::MarketPrice>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id \
         FROM market_prices ORDER BY instrument_id, priced_at DESC",
        [],
    )
}

#[tauri::command]
pub fn create_market_price(
    db: tauri::State<'_, crate::db::DbState>,
    input: crate::models::MarketPriceInput,
) -> Result<String> {
    if input.price_cents <= 0 {
        return Err(AppError::Invalid("价格必须大于 0".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM market_prices WHERE instrument_id=?1",
            rusqlite::params![input.instrument_id],
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
        rusqlite::params![
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

#[tauri::command]
pub fn list_instruments(
    db: tauri::State<'_, crate::db::DbState>,
) -> Result<Vec<crate::models::Instrument>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id \
         FROM instruments ORDER BY symbol",
        [],
    )
}

#[tauri::command]
pub fn create_instrument(
    db: tauri::State<'_, crate::db::DbState>,
    input: crate::models::InstrumentInput,
) -> Result<String> {
    if input.symbol.trim().is_empty() {
        return Err(AppError::Invalid("标的代码不能为空".into()));
    }
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            rusqlite::params![input.symbol, input.kind],
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
        rusqlite::params![
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, params};

    fn setup_db() -> Connection {
        let mut conn = crate::db::open_in_memory().unwrap();
        crate::db::init_db(&mut conn).unwrap();
        conn
    }

    fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![id, name, kind, currency],
        ).unwrap();
    }

    fn insert_instrument(conn: &Connection, id: &str, symbol: &str, name: &str, currency: &str) {
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?3,?4,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![id, symbol, name, currency],
        ).unwrap();
    }

    fn make_buy_input(
        account_id: &str,
        instrument_id: &str,
        qty: f64,
        price: i64,
        fee: i64,
    ) -> TransactionInput {
        TransactionInput {
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
            quantity: Some(qty),
            price_cents: Some(price),
            fee_cents: Some(fee),
        }
    }

    fn make_sell_input(
        account_id: &str,
        instrument_id: &str,
        qty: f64,
        price: i64,
        fee: i64,
    ) -> TransactionInput {
        TransactionInput {
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
            quantity: Some(qty),
            price_cents: Some(price),
            fee_cents: Some(fee),
        }
    }

    #[test]
    fn buy_transaction_creates_lot() {
        let conn = setup_db();
        insert_account(&conn, "acc-test-buy", "美股", "investment", "USD");
        insert_instrument(&conn, "inst-test-nvda", "NVDA", "NVIDIA", "USD");

        let input = make_buy_input("acc-test-buy", "inst-test-nvda", 10.0, 10000, 500);
        let txn_id = create_buy_transaction(&conn, input).unwrap();

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
        assert_eq!(cost_per_unit, 10050);

        let (holding_quantity, cost_basis): (f64, i64) = conn
            .query_row(
                "SELECT quantity, cost_basis_cents FROM v_holdings WHERE id=?1",
                params![format!("acc-test-buy-inst-test-nvda-USD")],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!((holding_quantity - 10.0).abs() < 0.0001);
        assert_eq!(cost_basis, 100500);
    }

    #[test]
    fn buy_transaction_requires_investment_account() {
        let conn = setup_db();
        insert_account(&conn, "acc-test-cash", "现金", "cash", "CNY");
        insert_instrument(&conn, "inst-test-cny", "600519", "茅台", "CNY");

        let input = make_buy_input("acc-test-cash", "inst-test-cny", 1.0, 10000, 0);
        assert!(create_buy_transaction(&conn, input).is_err());
    }

    #[test]
    fn sell_transaction_matches_multiple_lots_fifo() {
        let conn = setup_db();
        insert_account(&conn, "acc-test-sell", "美股", "investment", "USD");
        insert_instrument(&conn, "inst-test-sell", "TSLA", "Tesla", "USD");

        let lot1_txn = create_buy_transaction(
            &conn,
            make_buy_input("acc-test-sell", "inst-test-sell", 10.0, 10000, 0),
        )
        .unwrap();
        let lot2_txn = create_buy_transaction(
            &conn,
            make_buy_input("acc-test-sell", "inst-test-sell", 5.0, 12000, 0),
        )
        .unwrap();

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

        let sell_txn = create_sell_transaction(
            &conn,
            make_sell_input("acc-test-sell", "inst-test-sell", 12.0, 15000, 600),
        )
        .unwrap();

        let (kind, amount_cents): (String, i64) = conn
            .query_row(
                "SELECT kind, amount_cents FROM transactions WHERE id=?1",
                params![sell_txn],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(kind, "sell");
        assert_eq!(amount_cents, 179400);

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
        assert!((rows[0].0 - 10.0).abs() < 0.0001);
        assert_eq!(rows[0].1, 10000);
        assert_eq!(rows[0].2, 49500);
        assert_eq!(rows[0].3, "USD");
        assert!((rows[1].0 - 2.0).abs() < 0.0001);
        assert_eq!(rows[1].1, 12000);
        assert_eq!(rows[1].2, 5900);
        assert_eq!(rows[1].3, "USD");
    }

    #[test]
    fn sell_transaction_rejects_oversell() {
        let conn = setup_db();
        insert_account(&conn, "acc-test-oversell", "美股", "investment", "USD");
        insert_instrument(&conn, "inst-test-oversell", "MSFT", "Microsoft", "USD");

        create_buy_transaction(
            &conn,
            make_buy_input("acc-test-oversell", "inst-test-oversell", 5.0, 10000, 0),
        )
        .unwrap();

        let sell = make_sell_input("acc-test-oversell", "inst-test-oversell", 6.0, 12000, 0);
        assert!(create_sell_transaction(&conn, sell).is_err());
    }

    #[test]
    fn sell_transaction_pnl_deducts_fee() {
        let conn = setup_db();
        insert_account(&conn, "acc-test-pnl", "美股", "investment", "USD");
        insert_instrument(&conn, "inst-test-pnl", "AAPL", "Apple", "USD");

        let buy_txn = create_buy_transaction(
            &conn,
            make_buy_input("acc-test-pnl", "inst-test-pnl", 10.0, 10000, 0),
        )
        .unwrap();
        let sell_txn = create_sell_transaction(
            &conn,
            make_sell_input("acc-test-pnl", "inst-test-pnl", 5.0, 12000, 200),
        )
        .unwrap();

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
        assert_eq!(pnl, 9800);
        assert_eq!(ccy, "USD");

        let amount_cents: i64 = conn
            .query_row(
                "SELECT amount_cents FROM transactions WHERE id=?1",
                params![sell_txn],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(amount_cents, 5 * 12000 - 200);
    }

    // ---- instruments CRUD tests ----

    #[test]
    fn list_instruments_empty_initially() {
        let conn = setup_db();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn create_instrument_inserts_and_returns_id() {
        let conn = setup_db();
        let id = crate::db::new_uuid();
        let now = crate::db::now_iso();
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,'stock',?3,?4,?5,?6,?7,?8)",
            params![id, "NVDA", "NVIDIA Corporation", "USD", now, now, 1, "test"],
        ).unwrap();
        let (symbol, name, ccy): (String, Option<String>, String) = conn
            .query_row(
                "SELECT symbol, name, currency_code FROM instruments WHERE id=?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(symbol, "NVDA");
        assert_eq!(name.as_deref(), Some("NVIDIA Corporation"));
        assert_eq!(ccy, "USD");
    }

    #[test]
    fn create_instrument_is_idempotent() {
        let conn = setup_db();
        let id1 = crate::db::new_uuid();
        let id2 = crate::db::new_uuid();
        let now = crate::db::now_iso();
        // Insert first
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'AAPL','stock',?2,'USD',?3,?4,?5,?6)",
            params![id1, "Apple Inc.", now, now, 1, "test"],
        ).unwrap();
        // Attempt duplicate (same symbol+type) — should be rejected by UNIQUE constraint
        let result = conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'AAPL','stock',?2,'USD',?3,?4,?5,?6)",
            params![id2, "Apple Again", now, now, 1, "test"],
        );
        assert!(result.is_err());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM instruments WHERE symbol='AAPL' AND instrument_type='stock'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    // ---- holdings tests ----

    #[test]
    fn list_holdings_returns_after_buy_and_market_price() {
        let conn = setup_db();
        insert_account(&conn, "acc-hold", "投资账户", "investment", "USD");
        insert_instrument(&conn, "inst-hold", "GOOGL", "Alphabet", "USD");

        // Buy 10 shares at $150, fee $10
        let buy_input = make_buy_input("acc-hold", "inst-hold", 10.0, 15000, 1000);
        create_buy_transaction(&conn, buy_input).unwrap();

        // Insert market price
        let now = crate::db::now_iso();
        let price_id = crate::db::new_uuid();
        conn.execute(
            "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,16000,'USD',?3,NULL,?4,?5,?6,?7)",
            params![price_id, "inst-hold", now, now, now, 1, "test"],
        ).unwrap();

        // Query v_holdings view
        let (qty, cost_basis, market_value, unrealized_pnl): (f64, i64, i64, i64) = conn
            .query_row(
                "SELECT quantity, cost_basis_cents, market_value_cents, unrealized_pnl_cents \
                 FROM v_holdings WHERE instrument_id=?1",
                params!["inst-hold"],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!((qty - 10.0).abs() < 0.0001);
        assert_eq!(cost_basis, 151000); // 10 * 15000 + 1000
        assert_eq!(market_value, 160000); // 10 * 16000
        assert_eq!(unrealized_pnl, 9000); // 160000 - 151000
    }

    // ---- realized_pnl_summary tests ----

    fn empty_filter() -> PnlFilter {
        PnlFilter {
            account_id: None,
            instrument_id: None,
        }
    }

    #[test]
    fn realized_pnl_summary_empty_when_no_sales() {
        let conn = setup_db();
        let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
        assert_eq!(result.total_realized_pnl_cents, 0);
        assert!(result.by_year.is_empty());
        assert!(result.by_account.is_empty());
        assert!(result.by_instrument.is_empty());
        assert!(result.details.is_empty());
    }

    #[test]
    fn realized_pnl_summary_aggregates_single_sale() {
        let conn = setup_db();
        insert_account(&conn, "acc-pnl", "美股账户", "investment", "USD");
        insert_instrument(&conn, "inst-pnl", "AAPL", "Apple", "USD");

        let _buy =
            create_buy_transaction(&conn, make_buy_input("acc-pnl", "inst-pnl", 10.0, 10000, 0))
                .unwrap();
        let _sell = create_sell_transaction(
            &conn,
            make_sell_input("acc-pnl", "inst-pnl", 5.0, 12000, 200),
        )
        .unwrap();

        let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

        assert_eq!(result.total_realized_pnl_cents, 9800);
        assert_eq!(result.by_year.len(), 1);
        assert_eq!(result.by_year[0].realized_pnl_cents, 9800);
        assert_eq!(result.by_account.len(), 1);
        assert_eq!(result.by_account[0].account_id, "acc-pnl");
        assert_eq!(result.by_account[0].realized_pnl_cents, 9800);
        assert_eq!(result.by_instrument.len(), 1);
        assert_eq!(result.by_instrument[0].instrument_id, "inst-pnl");
        assert_eq!(result.by_instrument[0].symbol, "AAPL");
        assert_eq!(result.by_instrument[0].realized_pnl_cents, 9800);
        assert_eq!(result.details.len(), 1);
        assert_eq!(result.details[0].instrument_symbol, "AAPL");
        assert_eq!(result.details[0].quantity, 5.0);
        assert_eq!(result.details[0].realized_pnl_cents, 9800);
    }

    #[test]
    fn realized_pnl_summary_aggregates_multiple_accounts() {
        let conn = setup_db();
        insert_account(&conn, "acc-a", "账户A", "investment", "USD");
        insert_account(&conn, "acc-b", "账户B", "investment", "USD");
        insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

        create_buy_transaction(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 1000, 0)).unwrap();
        create_buy_transaction(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 2000, 0)).unwrap();
        create_sell_transaction(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 1500, 0)).unwrap();
        create_sell_transaction(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 2500, 0)).unwrap();

        let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

        assert_eq!(result.total_realized_pnl_cents, 3000);
        assert_eq!(result.by_account.len(), 2);
        assert_eq!(result.by_account[0].account_id, "acc-a");
        assert_eq!(result.by_account[0].realized_pnl_cents, 2000);
        assert_eq!(result.by_account[1].account_id, "acc-b");
        assert_eq!(result.by_account[1].realized_pnl_cents, 1000);
        assert_eq!(result.details.len(), 2);
    }

    #[test]
    fn realized_pnl_summary_filter_by_account() {
        let conn = setup_db();
        insert_account(&conn, "acc-a", "账户A", "investment", "USD");
        insert_account(&conn, "acc-b", "账户B", "investment", "USD");
        insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

        create_buy_transaction(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 1000, 0)).unwrap();
        create_buy_transaction(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 2000, 0)).unwrap();
        create_sell_transaction(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 1500, 0)).unwrap();
        create_sell_transaction(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 2500, 0)).unwrap();

        let filter = PnlFilter {
            account_id: Some("acc-a".into()),
            instrument_id: None,
        };
        let result = query_realized_pnl_summary(&conn, &filter).unwrap();

        assert_eq!(result.total_realized_pnl_cents, 2000);
        assert_eq!(result.by_account.len(), 1);
        assert_eq!(result.details.len(), 1);
    }

    #[test]
    fn realized_pnl_summary_filter_by_instrument() {
        let conn = setup_db();
        insert_account(&conn, "acc-pnl", "美股", "investment", "USD");
        insert_instrument(&conn, "inst-a", "AAPL", "Apple", "USD");
        insert_instrument(&conn, "inst-b", "GOOGL", "Alphabet", "USD");

        create_buy_transaction(&conn, make_buy_input("acc-pnl", "inst-a", 10.0, 1000, 0)).unwrap();
        create_buy_transaction(&conn, make_buy_input("acc-pnl", "inst-b", 5.0, 2000, 0)).unwrap();
        create_sell_transaction(&conn, make_sell_input("acc-pnl", "inst-a", 4.0, 1500, 0)).unwrap();
        create_sell_transaction(&conn, make_sell_input("acc-pnl", "inst-b", 2.0, 2500, 0)).unwrap();

        let filter = PnlFilter {
            account_id: None,
            instrument_id: Some("inst-a".into()),
        };
        let result = query_realized_pnl_summary(&conn, &filter).unwrap();

        assert_eq!(result.total_realized_pnl_cents, 2000);
        assert_eq!(result.by_instrument.len(), 1);
        assert_eq!(result.by_instrument[0].instrument_id, "inst-a");
        assert_eq!(result.details.len(), 1);
    }
}
