//! 币种清单查询（#404 自命令壳层迁入）。

use rusqlite::Connection;

use crate::db::query::query_all;
use crate::error::Result;

use super::model::Currency;

/// 币种清单：全部种子币种按 `code` 排序（参考数据只读，无软删/版本语义）。
/// IPC 与 HTTP 端点共用本函数。
pub fn list_currencies(conn: &Connection) -> Result<Vec<Currency>> {
    query_all(
        conn,
        "SELECT code,name,symbol,decimal_places FROM currencies ORDER BY code",
        [],
    )
}
