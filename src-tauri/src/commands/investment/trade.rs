use rusqlite::Connection;

use crate::commands::fx::account_currency_code;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{AccountType, TransactionInput};

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
