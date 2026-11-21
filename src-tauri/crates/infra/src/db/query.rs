use rusqlite::Connection;

use crate::error::Result;

pub trait FromRow: Sized {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self>;
}

pub fn query_all<T, P>(conn: &Connection, sql: &str, params: P) -> Result<Vec<T>>
where
    T: FromRow,
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| T::from_row(row))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[allow(dead_code)]
pub fn query_one<T, P>(conn: &Connection, sql: &str, params: P) -> Result<Option<T>>
where
    T: FromRow,
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query_map(params, |row| T::from_row(row))?;
    match rows.next() {
        Some(Ok(val)) => Ok(Some(val)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}
