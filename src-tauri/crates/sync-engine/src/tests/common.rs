use rusqlite::Connection;

use crate::{DirectConn, ingest_ops, read_ops};
use ledger_transaction::TransactionInput;
use ledger_transaction::amount::{FxRateSource, TransactionKind};
use tauri_app_lib::test_support::FIXED_NOW;

pub(crate) fn direct(conn: &Connection) -> DirectConn<'_> {
    DirectConn::new(conn)
}

pub(crate) fn wire_out(conn: &Connection) -> Vec<String> {
    read_ops(conn)
        .unwrap()
        .iter()
        .map(|op| serde_json::to_string(op).unwrap())
        .collect()
}

pub(crate) fn wire_in(conn: &Connection, wire: &[String]) -> Vec<crate::ApplyReport> {
    ingest_ops(conn, wire).unwrap()
}

pub(crate) fn make_expense(account_id: &str, amount_cents: i64, note: &str) -> TransactionInput {
    TransactionInput {
        funding: Vec::new(),
        merchant_name: None,
        policy_id: None,
        kind: TransactionKind::Expense,
        amount_cents,
        currency_code: "CNY".into(),
        account_id: Some(account_id.into()),
        to_account_id: None,
        funding_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        source_order_no: None,
        note: Some(note.into()),
        date: "2026-01-10".into(),
        instrument_id: None,
        quantity: None,
        price_cents: None,
        fee_cents: None,
        to_instrument_id: None,
        to_quantity: None,
        out_amount_cents: None,
        in_amount_cents: None,
        idempotency_key: None,
        origin: None,
        fx_rate: None,
        purchases: Vec::new(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TxnRow {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub fx_rate_used: Option<f64>,
    pub fx_rate_source: Option<FxRateSource>,
    pub account_id: String,
    pub to_account_id: Option<String>,
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub date: String,
    pub is_deleted: i64,
}

pub(crate) fn read_transaction(conn: &Connection, id: &str) -> Option<TxnRow> {
    conn.query_row(
        "SELECT kind, amount_cents, currency_code, amount_native_cents, account_id, \
         to_account_id, funding_account_id, category_id, merchant_id, refund_of_transaction_id, note, date, is_deleted, fx_rate_used, fx_rate_source \
         FROM transactions WHERE id = ?1",
        [id],
        |r| {
            Ok(TxnRow {
                kind: r.get(0)?,
                amount_cents: r.get(1)?,
                currency_code: r.get(2)?,
                amount_native_cents: r.get(3)?,
                account_id: r.get(4)?,
                to_account_id: r.get(5)?,
                funding_account_id: r.get(6)?,
                category_id: r.get(7)?,
                merchant_id: r.get(8)?,
                refund_of_transaction_id: r.get(9)?,
                note: r.get(10)?,
                date: r.get(11)?,
                is_deleted: r.get(12)?,
                fx_rate_used: r.get(13)?,
                fx_rate_source: r.get(14)?,
            })
        },
    )
    .ok()
}

pub(crate) fn read_lot(conn: &Connection, anchor_tx_id: &str) -> Option<(f64, f64, i64, String)> {
    conn.query_row(
        "SELECT initial_quantity, remaining_quantity, cost_per_unit_cents, currency_code \
         FROM security_lots WHERE buy_transaction_id = ?1",
        [anchor_tx_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .ok()
}

pub(crate) fn read_lot_sale(
    conn: &Connection,
    sell_tx_id: &str,
    anchor_tx_id: &str,
) -> Option<(f64, i64, i64)> {
    conn.query_row(
        "SELECT s.quantity, s.cost_per_unit_cents, s.realized_pnl_cents \
         FROM security_lot_sales s JOIN security_lots l ON l.id = s.lot_id \
         WHERE s.sell_transaction_id = ?1 AND l.buy_transaction_id = ?2",
        [sell_tx_id, anchor_tx_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

pub(crate) fn seed_plan(conn: &Connection, plan_id: &str, account_id: &str, amount_cents: i64) {
    conn.execute(
        "INSERT INTO scheduled_transactions \
         (id,kind,status,account_id,category_id,amount_cents,currency_code,\
         recurrence_type,recurrence_interval,recurrence_day,start_date,note,\
         created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'subscription','active',?2,NULL,?3,'CNY','monthly',1,NULL,'2026-01-01',NULL,?4,?4,1,'test',0)",
        rusqlite::params![plan_id, account_id, amount_cents, FIXED_NOW],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO subscription_plans (scheduled_transaction_id,merchant_id,policy_id) \
         VALUES (?1,NULL,NULL)",
        [plan_id],
    )
    .unwrap();
}

pub(crate) fn seed_occurrence(
    conn: &Connection,
    occ_id: &str,
    plan_id: &str,
    scheduled_date: &str,
    amount_cents: i64,
) {
    conn.execute(
        "INSERT INTO scheduled_transaction_occurrences \
         (id,scheduled_transaction_id,scheduled_date,status,transaction_id,amount_cents,\
         created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,'pending',NULL,?4,?5,?5,1,'test',0)",
        rusqlite::params![occ_id, plan_id, scheduled_date, amount_cents, FIXED_NOW],
    )
    .unwrap();
}

pub(crate) fn seed_device(conn: &Connection, device_id: &str) {
    conn.execute(
        "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) VALUES (?1, 0, ?2, ?2)",
        rusqlite::params![device_id, FIXED_NOW],
    )
    .unwrap();
}

pub(crate) fn read_occurrence(conn: &Connection, occ_id: &str) -> Option<(String, Option<String>)> {
    conn.query_row(
        "SELECT status, transaction_id FROM scheduled_transaction_occurrences WHERE id = ?1",
        [occ_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::transport::Transport;

#[derive(Default)]
pub(crate) struct MemoryTransport {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemoryTransport {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn files_remove(&self, path: &str) {
        self.files.lock().unwrap().remove(path);
    }
}

impl Transport for MemoryTransport {
    fn ensure_dir(&self, _path: &str) -> ledger_infra::error::Result<()> {
        Ok(())
    }

    fn read_file(&self, path: &str) -> ledger_infra::error::Result<Option<Vec<u8>>> {
        Ok(self.files.lock().unwrap().get(path).cloned())
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> ledger_infra::error::Result<()> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), bytes.to_vec());
        Ok(())
    }
}
