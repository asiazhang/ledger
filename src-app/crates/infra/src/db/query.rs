use rusqlite::Connection;

use crate::error::Result;

/// 从 SQLite 行反序列化结构体的 trait。对应 `query_all` / `query_one` 使用。
pub trait FromRow: Sized {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self>;
}

/// 执行查询并返回所有结果行。
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

/// 执行查询并返回至多一行结果。
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
