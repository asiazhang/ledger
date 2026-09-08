//! db 测试目录内共享脚手架：仅限本测试目录各子模块使用（跨测试模块合并不在此列，见 #250）。
//!
//! 建库与种子经统一测试工厂（spec #728 / issue #754 / ADR-0084 决策 7）：普通
//! 内存库用 [`crate::test_support::open`]；instrument/price/fx 种子已上收工厂
//! （`seed_instrument` / `seed_price_history` / `seed_fx_rate_history`）。本薄皮
//! 只留 db 域特有编排：schema 约束探针（准入规则：单域特有留域薄皮，
//! ADR-0084 决策 1）。

use std::sync::{Arc, Mutex};

use rusqlite::{Connection, params};

use crate::db::DbState;

/// 构造带 Arc<Mutex<Connection>> 的 DbState（写入口持锁形态）；建库经统一测试工厂。
pub(super) fn write_test_state() -> DbState {
    DbState {
        conn: Arc::new(Mutex::new(crate::test_support::open())),
    }
}

/// 读回自动备份调度状态（断言置脏语义用）。
pub(super) fn dirty_state(state: &DbState) -> crate::backup::AutoBackupState {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    crate::backup::get_state(&conn).expect("读调度状态")
}

// ---------------------------------------------------------------------------
// schema 约束探针（域特有）：断言「INSERT 被库层拒绝」必须直接写行——工厂种子
// 只产合法行、且 id 由货币对/调用方派生，表达不了约束违例形态。探针集中本薄皮
// （守门规则 2 对薄皮豁免，ADR-0084 决策 1）；返回 Result 供断言失败分支。
// ---------------------------------------------------------------------------

/// 探针：直写一行 exchange_rates（自定义 id，供货币对唯一约束探测）。
pub(super) fn probe_exchange_rate(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    rate: f64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,'2026-01-01','manual',?5,1,'test')",
        params![id, base, quote, rate, crate::test_support::FIXED_NOW],
    )
}

/// 探针：直写一行 price_history（自定义 id 与价格，供周唯一约束探测）。
pub(super) fn probe_price_history(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,'CNY','eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, instrument_id, trade_date, price_cents],
    )
}

/// 探针：直写一行 fx_rate_history（自定义 id 与汇率，供周唯一约束探测）。
pub(super) fn probe_fx_rate_history(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    trade_date: &str,
    rate: f64,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, base, quote, trade_date, rate],
    )
}

/// 探针：直写一行 instruments（name 显式 NULL、不带 source 列——最小合法形状，
/// 供 market CHECK 闭集探测；币种/时刻沿原探针值，为簿记非行为输入）。
pub(super) fn probe_instrument_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    market: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'stock',NULL,'USD',?3,'2026-09-01T00:00:00Z','2026-09-01T00:00:00Z',1,'test')",
        params![id, symbol, market],
    )
}

/// 探针：直写一行 instruments（source 列显式取值/NULL，供 NOT NULL 约束探测）。
pub(super) fn probe_instrument_source(
    conn: &Connection,
    id: &str,
    symbol: &str,
    market: &str,
    source: Option<&str>,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES (?1,?2,'stock','万科A','CNY',?3,'2026-01-02T00:00:00Z','2026-01-02T00:00:00Z',1,'test',?4)",
        params![id, symbol, market, source],
    )
}
