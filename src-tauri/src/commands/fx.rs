use rusqlite::Connection;

use crate::error::Result;

/// 查询账户本位币代码。
pub(crate) fn account_currency_code(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

// 注：`exchange_rate` 与 `convert_to_native`（账户币种基准的旧口径）已随 issue #60
// 接线删除——本位币折算统一走 `transaction::amount` 接缝（全局默认币种基准 +
// 正反向汇率兜底，语义见 transaction/tests.rs）；本模块仅保留投资层还在用的
// `account_currency_code`。
