use rusqlite::Connection;

use crate::error::Result;

pub fn ensure_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    if !conn.is_autocommit() {
        return f();
    }
    hold_transaction(conn, f)
}

pub fn hold_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    conn.execute("BEGIN", [])?;
    match f() {
        Ok(v) => match conn.execute("COMMIT", []) {
            Ok(_) => Ok(v),
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e.into())
            }
        },
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
        }
    }
}

pub fn rollback_if_open(conn: &Connection) -> Result<()> {
    if !conn.is_autocommit() {
        conn.execute("ROLLBACK", [])?;
    }
    if !conn.is_autocommit() {
        return Err(crate::error::AppError::Db(
            "回滚后连接仍未回到提交点，连接状态不可信".into(),
        ));
    }
    Ok(())
}
