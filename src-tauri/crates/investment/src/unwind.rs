use rusqlite::Connection;

use super::lots;
use super::split;
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::TransactionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Update,
    Delete,
}

const PARTIAL_SOLD_CANNOT_UPDATE: &str = "该买入交易已有部分卖出，无法修改";
const PARTIAL_SOLD_CANNOT_UPDATE_CODE: &str = "trade.partially-sold-update";

const CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE: &str = "该转换的转入份额已被后续卖出，无法修改";
const CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE_CODE: &str = "trade.convert-partially-sold-update";

const CONSUMED_BY_CONVERT_CANNOT_UPDATE: &str = "该交易的份额已被后续转换消耗，无法修改";
const CONSUMED_BY_CONVERT_CANNOT_UPDATE_CODE: &str = "trade.consumed-by-convert-update";
const CONSUMED_BY_CONVERT_CANNOT_DELETE: &str = "该交易的份额已被后续转换消耗，无法删除";
const CONSUMED_BY_CONVERT_CANNOT_DELETE_CODE: &str = "trade.consumed-by-convert-delete";

const CONSUMED_BY_SPLIT_CANNOT_UPDATE: &str = "该交易的份额已被后续份额调整重述，无法修改";
const CONSUMED_BY_SPLIT_CANNOT_UPDATE_CODE: &str = "trade.consumed-by-split-update";
const CONSUMED_BY_SPLIT_CANNOT_DELETE: &str = "该交易的份额已被后续份额调整重述，无法删除";
const CONSUMED_BY_SPLIT_CANNOT_DELETE_CODE: &str = "trade.consumed-by-split-delete";

const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE: &str =
    "该份额调整的批次已被后续交易消耗，无法修改";
const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE_CODE: &str = "trade.split-consumed-update";
const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE: &str =
    "该份额调整的批次已被后续交易消耗，无法删除";
const SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE_CODE: &str = "trade.split-consumed-delete";

pub fn remove(
    conn: &Connection,
    id: &str,
    kind: TransactionKind,
    mode: Mode,
) -> Result<Vec<String>> {
    match kind {
        TransactionKind::Sell => {
            lots::restore_sell(conn, id)?;
            Ok(Vec::new())
        }
        TransactionKind::Buy => match mode {
            Mode::Update => {
                guard_no_convert_consumed(conn, id, Mode::Update)?;
                guard_no_split_restated(conn, id, Mode::Update)?;
                guard_no_active_sell(
                    conn,
                    id,
                    PARTIAL_SOLD_CANNOT_UPDATE_CODE,
                    PARTIAL_SOLD_CANNOT_UPDATE,
                )?;
                purge_lot_artifacts(conn, id)?;
                Ok(Vec::new())
            }
            Mode::Delete => {
                guard_no_convert_consumed(conn, id, Mode::Delete)?;
                guard_no_split_restated(conn, id, Mode::Delete)?;
                let cascaded_sell_ids = cascade_active_sells(conn, id)?;
                purge_lot_artifacts(conn, id)?;
                Ok(cascaded_sell_ids)
            }
        },
        TransactionKind::Convert => match mode {
            Mode::Update => {
                guard_no_convert_consumed(conn, id, Mode::Update)?;
                guard_no_split_restated(conn, id, Mode::Update)?;
                guard_no_active_sell(
                    conn,
                    id,
                    CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE_CODE,
                    CONVERT_PARTIALLY_SOLD_CANNOT_UPDATE,
                )?;
                lots::restore_convert_out_leg(conn, id)?;
                purge_lot_artifacts(conn, id)?;
                Ok(Vec::new())
            }
            Mode::Delete => {
                guard_no_convert_consumed(conn, id, Mode::Delete)?;
                guard_no_split_restated(conn, id, Mode::Delete)?;
                let cascaded_sell_ids = cascade_active_sells(conn, id)?;
                lots::restore_convert_out_leg(conn, id)?;
                purge_lot_artifacts(conn, id)?;
                Ok(cascaded_sell_ids)
            }
        },
        TransactionKind::Split => {
            guard_no_split_downstream_consumed(conn, id, mode)?;
            split::restore_restatement(conn, id)?;
            Ok(Vec::new())
        }
        TransactionKind::Dividend => {
            conn.execute(
                "DELETE FROM security_transactions WHERE transaction_id=?1 AND action='dividend'",
                rusqlite::params![id],
            )?;
            Ok(Vec::new())
        }
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => Ok(Vec::new()),
    }
}

fn cascade_active_sells(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let sell_ids = active_sell_ids_on_own_lots(conn, anchor_id)?;
    for sell_id in &sell_ids {
        lots::restore_sell(conn, sell_id)?;
    }
    Ok(sell_ids)
}

fn active_sell_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT s.sell_transaction_id FROM security_lot_sales s \
         JOIN transactions t ON t.id = s.sell_transaction_id \
         WHERE t.is_deleted = 0 AND s.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn active_convert_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT c.transaction_id FROM security_lot_conversions c \
         JOIN transactions t ON t.id = c.transaction_id \
         WHERE t.is_deleted = 0 AND c.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn guard_no_convert_consumed(conn: &Connection, id: &str, mode: Mode) -> Result<()> {
    let (code, msg) = match mode {
        Mode::Update => (
            CONSUMED_BY_CONVERT_CANNOT_UPDATE_CODE,
            CONSUMED_BY_CONVERT_CANNOT_UPDATE,
        ),
        Mode::Delete => (
            CONSUMED_BY_CONVERT_CANNOT_DELETE_CODE,
            CONSUMED_BY_CONVERT_CANNOT_DELETE,
        ),
    };
    if !active_convert_ids_on_own_lots(conn, id)?.is_empty() {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

fn guard_no_split_restated(conn: &Connection, id: &str, mode: Mode) -> Result<()> {
    let (code, msg) = match mode {
        Mode::Update => (
            CONSUMED_BY_SPLIT_CANNOT_UPDATE_CODE,
            CONSUMED_BY_SPLIT_CANNOT_UPDATE,
        ),
        Mode::Delete => (
            CONSUMED_BY_SPLIT_CANNOT_DELETE_CODE,
            CONSUMED_BY_SPLIT_CANNOT_DELETE,
        ),
    };
    if !active_split_ids_on_own_lots(conn, id)?.is_empty() {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

fn active_split_ids_on_own_lots(conn: &Connection, anchor_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT a.transaction_id FROM security_lot_adjustments a \
         JOIN transactions t ON t.id = a.transaction_id \
         WHERE t.is_deleted = 0 AND a.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
    )?;
    let ids = stmt
        .query_map(rusqlite::params![anchor_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn guard_no_split_downstream_consumed(conn: &Connection, id: &str, mode: Mode) -> Result<()> {
    let (code, msg) = match mode {
        Mode::Update => (
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE_CODE,
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_UPDATE,
        ),
        Mode::Delete => (
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE_CODE,
            SPLIT_CONSUMED_BY_DOWNSTREAM_CANNOT_DELETE,
        ),
    };
    let consumed: bool = conn.query_row(
        "WITH adjusted(lot_id) AS ( \
           SELECT lot_id FROM security_lot_adjustments WHERE transaction_id=?1 \
         ), downstream(txn_id) AS ( \
           SELECT s.sell_transaction_id FROM security_lot_sales s \
            WHERE s.lot_id IN (SELECT lot_id FROM adjusted) \
           UNION ALL \
           SELECT c.transaction_id FROM security_lot_conversions c \
            WHERE c.lot_id IN (SELECT lot_id FROM adjusted) \
           UNION ALL \
           SELECT a.transaction_id FROM security_lot_adjustments a \
            WHERE a.transaction_id<>?1 AND a.lot_id IN (SELECT lot_id FROM adjusted) \
         ) \
         SELECT EXISTS ( \
           SELECT 1 FROM downstream d \
           JOIN transactions t ON t.id = d.txn_id \
           WHERE t.is_deleted = 0 \
             AND t.rowid > (SELECT rowid FROM transactions WHERE id = ?1) \
         )",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if consumed {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}

fn purge_lot_artifacts(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM security_lot_sales WHERE lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id=?1)",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_lots WHERE buy_transaction_id=?1",
        rusqlite::params![id],
    )?;
    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id=?1",
        rusqlite::params![id],
    )?;
    Ok(())
}

fn guard_no_active_sell(conn: &Connection, id: &str, code: &str, msg: &str) -> Result<()> {
    let partially_sold: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT s.sell_transaction_id) FROM security_lot_sales s \
         JOIN transactions t ON t.id = s.sell_transaction_id \
         WHERE t.is_deleted = 0 AND s.lot_id IN \
         (SELECT id FROM security_lots WHERE buy_transaction_id = ?1)",
        rusqlite::params![id],
        |r| r.get(0),
    )?;
    if partially_sold > 0 {
        return Err(AppError::coded(code, msg));
    }
    Ok(())
}
