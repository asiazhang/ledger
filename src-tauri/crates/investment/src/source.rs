use rusqlite::Connection;

use super::model::InstrumentSourceDisplay;
use ledger_infra::db::query::query_all;
use ledger_infra::error::Result;

pub fn source_display_by_transaction_ids(
    conn: &Connection,
    transaction_ids: &[String],
) -> Result<Vec<InstrumentSourceDisplay>> {
    if transaction_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; transaction_ids.len()].join(",");
    query_all(
        conn,
        &format!(
            "SELECT st.transaction_id, st.instrument_id, i.symbol, i.name \
             FROM security_transactions st JOIN instruments i ON i.id = st.instrument_id \
             WHERE st.transaction_id IN ({placeholders})",
        ),
        rusqlite::params_from_iter(transaction_ids.iter()),
    )
}
