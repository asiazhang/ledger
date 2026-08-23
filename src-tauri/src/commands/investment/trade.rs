use rusqlite::Connection;

use crate::commands::fx::account_currency_code;
use crate::db::query::{FromRow, query_all};
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{AccountType, NormalizedTransaction, TransactionInput};

pub(crate) struct ActiveLot {
    pub(crate) id: String,
    pub(crate) remaining_quantity: f64,
    pub(crate) cost_per_unit_cents: i64,
    pub(crate) currency_code: String,
}

impl FromRow for ActiveLot {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(ActiveLot {
            id: row.get(0)?,
            remaining_quantity: row.get(1)?,
            cost_per_unit_cents: row.get(2)?,
            currency_code: row.get(3)?,
        })
    }
}

pub(crate) struct BuyPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
}

/// 校验并归一化一笔买入交易（不落库）。创建与修改共用；
/// 只做校验与字段解析，持仓建仓等副作用由调用方在落库时按其身份（新增或替换）执行。
pub(crate) fn prepare_buy(conn: &Connection, input: &TransactionInput) -> Result<BuyPlan> {
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

    Ok(BuyPlan {
        normalized: NormalizedTransaction {
            kind: "buy".into(),
            amount_cents,
            currency_code: account_currency,
            amount_native_cents: amount_cents,
            account_id: input.account_id.clone(),
            to_account_id: input.to_account_id.clone(),
            category_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
    })
}

pub(crate) fn create_buy_transaction(conn: &Connection, input: TransactionInput) -> Result<String> {
    let plan = prepare_buy(conn, &input)?;
    write_buy(conn, &plan)
}

fn write_buy(conn: &Connection, plan: &BuyPlan) -> Result<String> {
    let id = new_uuid();
    let now = now_iso();
    let norm = &plan.normalized;
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',?2,?3,?4,?5,?6,NULL,NULL,?7,?8,?9,?10,?11,?12,0)",
        rusqlite::params![
            id,
            norm.amount_cents,
            norm.currency_code,
            norm.amount_native_cents,
            norm.account_id,
            norm.to_account_id,
            norm.note,
            norm.date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    create_buy_lot(
        conn,
        &id,
        &norm.account_id,
        &plan.instrument_id,
        plan.quantity,
        plan.price_cents,
        plan.fee_cents,
        &norm.currency_code,
    )?;
    Ok(id)
}

/// 校验并归一化一笔卖出交易（不落库）。创建与修改共用；
/// 卖出匹配持仓等副作用由调用方在落库时按其身份执行。
pub(crate) fn prepare_sell(conn: &Connection, input: &TransactionInput) -> Result<SellPlan> {
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

    let lots: Vec<ActiveLot> = query_all(
        conn,
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY created_at ASC, id ASC",
        rusqlite::params![input.account_id, instrument_id],
    )?;
    let total_available: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    if total_available < quantity {
        return Err(AppError::Invalid(format!(
            "可卖出数量不足，当前持有 {}，尝试卖出 {}",
            total_available, quantity
        )));
    }

    Ok(SellPlan {
        normalized: NormalizedTransaction {
            kind: "sell".into(),
            amount_cents,
            currency_code: account_currency,
            amount_native_cents: amount_cents,
            account_id: input.account_id.clone(),
            to_account_id: input.to_account_id.clone(),
            category_id: None,
            refund_of_transaction_id: None,
            note: input.note.clone(),
            date: input.date.clone(),
        },
        instrument_id,
        quantity,
        price_cents,
        fee_cents,
        lots,
    })
}

pub(crate) fn create_sell_transaction(
    conn: &Connection,
    input: TransactionInput,
) -> Result<String> {
    let plan = prepare_sell(conn, &input)?;
    write_sell(conn, &plan)
}

fn write_sell(conn: &Connection, plan: &SellPlan) -> Result<String> {
    let id = new_uuid();
    let now = now_iso();
    let norm = &plan.normalized;
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'sell',?2,?3,?4,?5,?6,NULL,NULL,?7,?8,?9,?10,?11,?12,0)",
        rusqlite::params![
            id,
            norm.amount_cents,
            norm.currency_code,
            norm.amount_native_cents,
            norm.account_id,
            norm.to_account_id,
            norm.note,
            norm.date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'sell',?3,?4,?5)",
        rusqlite::params![id, plan.instrument_id, plan.quantity, plan.price_cents, plan.fee_cents],
    )?;

    let mut remaining_to_sell = plan.quantity;
    let mut matched_lots: Vec<ActiveLot> = Vec::new();
    for lot in &plan.lots {
        if remaining_to_sell <= 0.0 {
            break;
        }
        let matched = lot.remaining_quantity.min(remaining_to_sell);
        matched_lots.push(ActiveLot {
            id: lot.id.clone(),
            remaining_quantity: matched,
            cost_per_unit_cents: lot.cost_per_unit_cents,
            currency_code: lot.currency_code.clone(),
        });
        remaining_to_sell -= matched;
    }

    let match_count = matched_lots.len();
    let mut allocated_fee_total = 0i64;
    for (i, lot) in matched_lots.iter().enumerate() {
        let lot_proceeds = (lot.remaining_quantity * plan.price_cents as f64).round() as i64;
        let lot_cost = (lot.remaining_quantity * lot.cost_per_unit_cents as f64).round() as i64;
        let allocated_fee = if i == match_count - 1 {
            plan.fee_cents - allocated_fee_total
        } else {
            let fee =
                (plan.fee_cents as f64 * lot.remaining_quantity / plan.quantity).floor() as i64;
            allocated_fee_total += fee;
            fee
        };
        let realized_pnl = lot_proceeds - lot_cost - allocated_fee;
        let sale_id = new_uuid();
        conn.execute(
            "INSERT INTO security_lot_sales (id,sell_transaction_id,lot_id,quantity,cost_per_unit_cents,realized_pnl_cents,currency_code,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            rusqlite::params![sale_id, id, lot.id, lot.remaining_quantity, lot.cost_per_unit_cents, realized_pnl, lot.currency_code, now],
        )?;
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity-?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![lot.remaining_quantity, now, device_id(), lot.id],
        )?;
    }

    Ok(id)
}

pub(crate) struct SellPlan {
    pub(crate) normalized: NormalizedTransaction,
    pub(crate) instrument_id: String,
    pub(crate) quantity: f64,
    pub(crate) price_cents: i64,
    pub(crate) fee_cents: i64,
    pub(crate) lots: Vec<ActiveLot>,
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
