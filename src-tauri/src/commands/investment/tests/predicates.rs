//! 「持仓标的」判定谓词一致性绑定测试（issue #239）：单点谓词
//! （`predicates::INVESTED_EXISTS`）选出的标的集合 ≡ `v_holdings` 视图有行的
//! 标的集合。视图定义随发布冻结、只增不改，是只读对账基准；两份 SQL 编码的
//! 一致性靠本测试钉住（先例：周键 ↔ week_start 生成列绑定测试），口径漂移
//! 在测试期即失败，而非上线后增量同步与标的页过滤分叉。
//!
//! 夹具覆盖父 spec #172 点名的四类分叉：软删除账户批次、已清仓标的、
//! 非股票持仓、同账户同标的不同币种 lot（视图 GROUP BY 含 currency_code，
//! 一标的出多行——按标的去重后必须与谓词集合逐标的相等）。

use rusqlite::{Connection, params};

use super::common::{insert_account, setup_db};
use crate::commands::investment::predicates::INVESTED_EXISTS;

/// 直插一个标的（类型可指定），绕过命令层以聚焦谓词集合本身。
fn insert_instrument_typed(conn: &Connection, id: &str, symbol: &str, kind: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,'CNY','sh','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, kind, format!("名称-{symbol}")],
    )
    .unwrap();
}

/// 直插一条最小买入链（transaction → security_transaction → lot）：谓词与视图
/// 的驱动表是 security_lots，补齐外键链即可，绕过交易行为层以聚焦本测试。
fn insert_lot(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    remaining: f64,
    lot_currency: &str,
) {
    let txn_id = format!("txn-{account_id}-{instrument_id}-{lot_currency}");
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',1000,'CNY',1000,?2,'2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![txn_id, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,100,0)",
        params![txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,?5,100,?6,'2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![
            format!("lot-{txn_id}"),
            account_id,
            instrument_id,
            txn_id,
            remaining,
            lot_currency
        ],
    )
    .unwrap();
}

fn soft_delete_account(conn: &Connection, id: &str) {
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id=?1", params![id])
        .unwrap();
}

/// 谓词选出的标的集合（按 id 升序）。外层查询以 `i` 作 instruments 别名——
/// 同时是对 predicates 模块别名契约的例行演练。
fn predicate_instrument_set(conn: &Connection) -> Vec<String> {
    let sql = format!("SELECT i.id FROM instruments i WHERE {INVESTED_EXISTS} ORDER BY i.id");
    let mut stmt = conn.prepare(&sql).unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

/// v_holdings 视图有行的标的集合（去重，按 id 升序）。
fn view_instrument_set(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT instrument_id FROM v_holdings ORDER BY instrument_id")
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

#[test]
fn invested_predicate_set_equals_v_holdings_instrument_set() {
    let conn = setup_db();

    // ① 普通在持标的：股票，正常账户 —— 两侧都应包含。
    insert_account(&conn, "acc-live", "在持账户", "investment", "CNY");
    insert_instrument_typed(&conn, "inst-stock", "600001", "stock");
    insert_lot(&conn, "acc-live", "inst-stock", 10.0, "CNY");

    // ② 非股票持仓（基金/债券）：数据源无行情，但「持仓标的」判定与类型无关
    //    （增量同步侧按类型分区计跳过，判定口径本身不分类型）——两侧都应包含。
    insert_instrument_typed(&conn, "inst-fund", "110011", "fund");
    insert_lot(&conn, "acc-live", "inst-fund", 100.0, "CNY");
    insert_instrument_typed(&conn, "inst-bond", "019547", "bond");
    insert_lot(&conn, "acc-live", "inst-bond", 10.0, "CNY");

    // ③ 已清仓标的：批次剩余数量为 0 ——「持仓标的」不含已清仓，两侧都应排除。
    insert_instrument_typed(&conn, "inst-cleared", "600002", "stock");
    insert_lot(&conn, "acc-live", "inst-cleared", 0.0, "CNY");

    // ④ 软删除账户的批次：口径明确排除 —— 两侧都应排除。
    insert_account(&conn, "acc-del", "已删账户", "investment", "CNY");
    insert_instrument_typed(&conn, "inst-softdel", "600003", "stock");
    insert_lot(&conn, "acc-del", "inst-softdel", 10.0, "CNY");
    soft_delete_account(&conn, "acc-del");

    // ⑤ 同账户同标的不同币种 lot：视图 GROUP BY 含 currency_code，一标的出两行；
    //    谓词按标的判定只出一次 —— 去重后两侧逐标的相等。
    insert_instrument_typed(&conn, "inst-multiccy", "600004", "stock");
    insert_lot(&conn, "acc-live", "inst-multiccy", 5.0, "CNY");
    insert_lot(&conn, "acc-live", "inst-multiccy", 3.0, "USD");

    // 谓词集合先对显式期望（防两侧同错的空集/同错集平凡通过）：
    // 在持 = stock + fund + bond + multiccy；排除 = cleared（清仓）+ softdel（软删账户）。
    assert_eq!(
        predicate_instrument_set(&conn),
        vec![
            "inst-bond".to_string(),
            "inst-fund".to_string(),
            "inst-multiccy".to_string(),
            "inst-stock".to_string(),
        ],
        "谓词集合应恰为四只在持标的（含非股票），排除清仓与软删账户批次"
    );

    // 核心绑定：谓词集合 ≡ 视图有行的标的集合（按标的去重）。
    assert_eq!(
        predicate_instrument_set(&conn),
        view_instrument_set(&conn),
        "单点谓词与 v_holdings 视图的标的口径漂移"
    );

    // 视图侧多行现实核查：multiccy 标的视图确有 2 行（不同成本币种），
    // 证明分叉场景真实进入了对账。
    let multiccy_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-multiccy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(multiccy_rows, 2, "同标的不同币种 lot 应在视图出两行");
}
