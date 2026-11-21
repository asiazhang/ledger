use rusqlite::Connection;

use super::guards;
use super::prices::PRICE_UNITS_PER_FEN;
use ledger_infra::db::now_iso;
use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::Result;
use ledger_sync_protocol::device::device_id;

pub(crate) const QTY_GUARD_EPSILON: f64 = 1e-6;

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

pub(crate) struct Consumption {
    pub(crate) lot: ActiveLot,
    pub(crate) cost_cents: i64,
}

pub(crate) fn active_lots(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
) -> Result<Vec<ActiveLot>> {
    query_all(
        conn,
        "SELECT id, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots \
         WHERE account_id=?1 AND instrument_id=?2 AND remaining_quantity > 0 \
         ORDER BY rowid ASC",
        rusqlite::params![account_id, instrument_id],
    )
}

pub(crate) fn prior_consumed_cost_cents(conn: &Connection, lot_id: &str) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT quantity, cost_per_unit_cents FROM security_lot_sales WHERE lot_id=?1 \
         UNION ALL \
         SELECT quantity, cost_per_unit_cents FROM security_lot_conversions WHERE lot_id=?1",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![lot_id], |r| {
            Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .iter()
        .map(|(q, recorded_cpu)| (q * *recorded_cpu as f64 / PRICE_UNITS_PER_FEN).round() as i64)
        .sum())
}

pub(crate) fn plan(
    conn: &Connection,
    lots: &[ActiveLot],
    quantity: f64,
) -> Result<Vec<Consumption>> {
    let total_available: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    if total_available + QTY_GUARD_EPSILON < quantity {
        return Err(guards::insufficient_holding_error(
            total_available,
            quantity,
        ));
    }
    let mut remaining = quantity;
    let mut out: Vec<Consumption> = Vec::new();
    for lot in lots {
        if remaining <= 0.0 {
            break;
        }
        let exhausts_lot = remaining + QTY_GUARD_EPSILON >= lot.remaining_quantity;
        let matched = if exhausts_lot {
            lot.remaining_quantity
        } else {
            remaining
        };
        let cost_cents = if exhausts_lot {
            let lot_total_cents: i64 = conn.query_row(
                "SELECT t.amount_cents FROM security_lots l \n                 JOIN transactions t ON t.id = l.buy_transaction_id WHERE l.id=?1",
                rusqlite::params![lot.id],
                |r| r.get(0),
            )?;
            lot_total_cents - prior_consumed_cost_cents(conn, &lot.id)?
        } else {
            (matched * lot.cost_per_unit_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64
        };
        out.push(Consumption {
            lot: ActiveLot {
                id: lot.id.clone(),
                remaining_quantity: matched,
                cost_per_unit_cents: lot.cost_per_unit_cents,
                currency_code: lot.currency_code.clone(),
            },
            cost_cents,
        });
        remaining -= matched;
    }
    Ok(out)
}

pub(crate) fn total_cost(consumed: &[Consumption]) -> i64 {
    consumed.iter().map(|c| c.cost_cents).sum()
}

pub(crate) fn restore_sell(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let mut stmt = conn
        .prepare("SELECT lot_id, quantity FROM security_lot_sales WHERE sell_transaction_id=?1")?;
    let sales: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in sales {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(conn)?, lot_id],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_sales WHERE sell_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='sell'",
        rusqlite::params![id],
    )?;
    Ok(())
}

pub(crate) fn restore_convert_out_leg(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let mut stmt = conn
        .prepare("SELECT lot_id, quantity FROM security_lot_conversions WHERE transaction_id=?1")?;
    let consumptions: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for (lot_id, quantity) in consumptions {
        conn.execute(
            "UPDATE security_lots SET remaining_quantity=remaining_quantity+?1, \
             updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
            rusqlite::params![quantity, now, device_id(conn)?, lot_id],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_conversions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}
