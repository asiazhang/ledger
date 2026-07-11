use rusqlite::Connection;

use crate::commands::fx::account_currency_code;
use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{AccountType, TransactionInput};

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
}
