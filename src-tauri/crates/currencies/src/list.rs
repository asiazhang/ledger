use rusqlite::Connection;

use ledger_infra::db::query::query_all;
use ledger_infra::error::Result;

use super::model::Currency;

pub fn list_currencies(conn: &Connection) -> Result<Vec<Currency>> {
    query_all(
        conn,
        "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
        [],
    )
}
