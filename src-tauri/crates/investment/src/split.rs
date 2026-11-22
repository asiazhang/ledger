use rusqlite::Connection;

use super::guards;
use super::lots::{self, QTY_GUARD_EPSILON};
use super::prices::PRICE_UNITS_PER_FEN;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::Result;
use ledger_sync_protocol::device::device_id;

pub(crate) struct LotRestatement {
    pub(crate) lot_id: String,
    pub(crate) initial_before: f64,
    pub(crate) initial_after: f64,
    pub(crate) remaining_before: f64,
    pub(crate) remaining_after: f64,
    pub(crate) cost_per_unit_before: i64,
    pub(crate) cost_per_unit_after: i64,
}

pub(crate) struct Restatement {
    pub(crate) lots: Vec<LotRestatement>,
    pub(crate) final_quantity: f64,
    pub(crate) total_cost_cents: i64,
}

struct LotBefore {
    lot_id: String,
    initial_quantity: f64,
    remaining_quantity: f64,
    cost_per_unit_cents: i64,
}

struct LotSnapshot {
    lot_id: String,
    initial_quantity: f64,
    remaining_quantity: f64,
    cost_per_unit_cents: i64,
    anchor_cents: i64,
}

fn snapshot_active_lots(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    before_transaction_rowid: Option<i64>,
) -> Result<Vec<LotSnapshot>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.initial_quantity, l.remaining_quantity, l.cost_per_unit_cents, t.amount_cents \
         FROM security_lots l \
         JOIN transactions t ON t.id = l.buy_transaction_id \
         WHERE l.account_id=?1 AND l.instrument_id=?2 AND l.remaining_quantity > 0 \
           AND (?3 IS NULL OR t.rowid < ?3) \
         ORDER BY l.rowid ASC",
    )?;
    let rows = stmt
        .query_map(
            rusqlite::params![account_id, instrument_id, before_transaction_rowid],
            |r| {
                Ok(LotSnapshot {
                    lot_id: r.get(0)?,
                    initial_quantity: r.get(1)?,
                    remaining_quantity: r.get(2)?,
                    cost_per_unit_cents: r.get(3)?,
                    anchor_cents: r.get(4)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn plan_restatement(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    delta: f64,
    before_transaction_rowid: Option<i64>,
) -> Result<Restatement> {
    let lots = snapshot_active_lots(conn, account_id, instrument_id, before_transaction_rowid)?;
    let total_holding: f64 = lots.iter().map(|l| l.remaining_quantity).sum();
    if delta < 0.0 && total_holding + delta <= QTY_GUARD_EPSILON {
        return Err(guards::shrink_not_less_than_holding_error(
            total_holding,
            -delta,
        ));
    }
    if lots.is_empty() {
        return Ok(Restatement {
            lots: Vec::new(),
            final_quantity: 0.0,
            total_cost_cents: 0,
        });
    }
    let factor = (total_holding + delta) / total_holding;

    let mut target_total_cents = 0i64;
    for lot in &lots {
        target_total_cents +=
            lot.anchor_cents - lots::prior_consumed_cost_cents(conn, &lot.lot_id)?;
    }

    let last = lots.len() - 1;
    let mut allocated_cents = 0.0f64;
    let mut out = Vec::with_capacity(lots.len());
    for (i, lot) in lots.iter().enumerate() {
        let remaining_after = lot.remaining_quantity * factor;
        let initial_after = lot.initial_quantity * factor;
        let cost_per_unit_after = if i < last {
            (lot.cost_per_unit_cents as f64 / factor).round() as i64
        } else {
            let closed = ((target_total_cents as f64 - allocated_cents) * PRICE_UNITS_PER_FEN
                / remaining_after)
                .round() as i64;
            closed.max(0)
        };
        allocated_cents += remaining_after * cost_per_unit_after as f64 / PRICE_UNITS_PER_FEN;
        out.push(LotRestatement {
            lot_id: lot.lot_id.clone(),
            initial_before: lot.initial_quantity,
            initial_after,
            remaining_before: lot.remaining_quantity,
            remaining_after,
            cost_per_unit_before: lot.cost_per_unit_cents,
            cost_per_unit_after,
        });
    }
    let final_quantity: f64 = out.iter().map(|r| r.remaining_after).sum();
    Ok(Restatement {
        lots: out,
        final_quantity,
        total_cost_cents: target_total_cents,
    })
}

pub(crate) fn write_split_side_effects(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    delta_quantity: f64,
    restated: &[LotRestatement],
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'split',?3,NULL,0)",
        rusqlite::params![id, instrument_id, delta_quantity],
    )?;
    for r in restated {
        conn.execute(
            "UPDATE security_lots SET initial_quantity=?2, remaining_quantity=?3, cost_per_unit_cents=?4, \
             updated_at=?5, version=version+1, device_id=?6 WHERE id=?1",
            rusqlite::params![
                r.lot_id,
                r.initial_after,
                r.remaining_after,
                r.cost_per_unit_after,
                now,
                device_id(conn)?
            ],
        )?;
        conn.execute(
            "INSERT INTO security_lot_adjustments \
             (id,transaction_id,lot_id,initial_quantity_before,initial_quantity_after,\
             remaining_quantity_before,remaining_quantity_after,cost_per_unit_cents_before,\
             cost_per_unit_cents_after,created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                new_uuid(),
                id,
                r.lot_id,
                r.initial_before,
                r.initial_after,
                r.remaining_before,
                r.remaining_after,
                r.cost_per_unit_before,
                r.cost_per_unit_after,
                now
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn restore_restatement(conn: &Connection, id: &str) -> Result<()> {
    let now = now_iso();
    let prior: Vec<LotBefore> = {
        let mut stmt = conn.prepare(
            "SELECT lot_id, initial_quantity_before, remaining_quantity_before, \
             cost_per_unit_cents_before FROM security_lot_adjustments WHERE transaction_id=?1",
        )?;
        stmt.query_map(rusqlite::params![id], |r| {
            Ok(LotBefore {
                lot_id: r.get(0)?,
                initial_quantity: r.get(1)?,
                remaining_quantity: r.get(2)?,
                cost_per_unit_cents: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };
    for lot in prior {
        conn.execute(
            "UPDATE security_lots SET initial_quantity=?2, remaining_quantity=?3, cost_per_unit_cents=?4, \
             updated_at=?5, version=version+1, device_id=?6 WHERE id=?1",
            rusqlite::params![
                lot.lot_id,
                lot.initial_quantity,
                lot.remaining_quantity,
                lot.cost_per_unit_cents,
                now,
                device_id(conn)?
            ],
        )?;
    }
    conn.execute(
        "DELETE FROM security_lot_adjustments WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}
