use rusqlite::{Connection, OptionalExtension, params};

use ledger_accounts::balance::{compute_balance, list_accounts_with_visibility};

pub fn extract_check_in_literals(ddl: &str, col: &str) -> Option<Vec<String>> {
    let needle = format!("CHECK({col} IN (");
    let start = ddl.find(&needle)? + needle.len();
    let rest = &ddl[start..];
    let end = rest.find("))")?;
    Some(
        rest[..end]
            .split(',')
            .map(|s| s.trim().trim_matches('\'').to_string())
            .collect(),
    )
}

pub fn read_scalar_i64(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Option<i64> {
    conn.query_row(sql, params, |r| r.get(0))
        .optional()
        .unwrap()
}

pub fn assert_balance_cache_matches_realtime(conn: &Connection) {
    for account in list_accounts_with_visibility(conn, true).unwrap() {
        let cached = read_scalar_i64(
            conn,
            "SELECT balance_cents FROM account_balance_cache WHERE account_id=?1",
            params![account.id],
        );
        assert_eq!(
            cached,
            Some(compute_balance(conn, &account.id).unwrap()),
            "账户 {}({}) 缓存应等于实时计算",
            account.id,
            account.name
        );
    }
}
