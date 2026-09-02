//! 行情同步测试共享脚手架：仅限本测试目录内各子模块使用（跨测试模块合并不在此列，见 #250）。

use rusqlite::Connection;

use crate::db::{init_db, open_in_memory};

pub(super) fn setup_db() -> Connection {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    conn
}
