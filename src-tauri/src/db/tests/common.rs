//! db 测试目录内共享脚手架：仅限本测试目录各子模块使用（跨测试模块合并不在此列，见 #250）。

use rusqlite::{Connection, params};

use crate::db::DbState;

/// 在测试库中创建一个可引用的金融工具（依赖 init_db 种子币种）。
pub(super) fn insert_instrument(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'600519.SH','stock','贵州茅台',?2,'sh','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, currency],
    )
    .unwrap();
}

/// 写入一条 price_history（周采样价格历史）。
pub(super) fn insert_price_history(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    trade_date: &str,
) {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,170000,'CNY','eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, instrument_id, trade_date],
    )
    .unwrap();
}

/// 写入一条 fx_rate_history（周采样汇率历史）。
pub(super) fn insert_fx_rate_history(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    trade_date: &str,
    rate: f64,
) {
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, base, quote, trade_date, rate],
    )
    .unwrap();
}

/// 构造带 Arc<Mutex<Connection>> 的 DbState（写入口持锁形态）。
pub(super) fn write_test_state() -> DbState {
    DbState::open_in_memory().expect("打开内存库")
}

/// 读回自动备份调度状态（断言置脏语义用）。
pub(super) fn dirty_state(state: &DbState) -> crate::auto_backup::AutoBackupState {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    crate::auto_backup::get_state(&conn).expect("读调度状态")
}
