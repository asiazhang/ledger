use super::*;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::params;
use tracing::Level;

use crate::test_utils::capture_events;
/// 校验迁移集合本身定义正确（在临时内存 DB 上从首到尾跑一遍向上迁移）。
#[test]
fn migrations_validate() {
    assert!(migrations().validate().is_ok());
}

/// init_db 应幂等：连续跑两次不报错，且默认币种 11 条、分类 92 条已写入
/// （18 顶级 + 74 二级）。
#[test]
fn init_db_is_idempotent_and_seeds_defaults() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    init_db(&mut conn).unwrap();

    let currency_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM currencies", [], |r| r.get(0))
        .unwrap();
    assert_eq!(currency_count, 11);

    let cat_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cat_count, 92);

    let root_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE parent_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(root_count, 18);

    // 每个二级分类的 parent_id 必须指向同 kind 的顶级分类。
    let mismatched: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM categories c \
             JOIN categories p ON p.id=c.parent_id \
             WHERE c.parent_id IS NOT NULL AND p.kind<>c.kind",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mismatched, 0);
}

/// 汇率表每货币对仅保留一行最新（UNIQUE(base_code, quote_code) 约束）。
/// 正反向查表与折算语义已收口到 Amount 接缝（`transaction::amount::convert_to_native`，
/// 见 transaction/tests.rs），此处不再重复。
#[test]
fn exchange_rate_single_row_per_pair() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,'USD','CNY',7.2,'2026-06-01','manual','2026-06-01T00:00:00Z',1,'test')",
        params!["er-01"],
    )
    .unwrap();

    // 同货币对第二行应被 UNIQUE(base_code, quote_code) 拒绝。
    let dup = conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,'USD','CNY',7.0,'2026-01-01','manual','2026-01-01T00:00:00Z',1,'test')",
        params!["er-02"],
    );
    assert!(dup.is_err(), "同货币对第二行应违反唯一约束");
}

/// 跨币种持仓：CNY 账户持 USD 标的，市值与成本都应折算到 CNY 后再相减。
/// 旧实现只折算市值、不折算成本，会把 CNY 市值直接减 USD 成本，结果错误。
#[test]
fn cross_currency_holding_pnl() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    let account_id = "acc-test-cny-inv";
    let instrument_id = "inst-test-nvda";
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'美股CNY','investment','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,'NVDA','stock','NVIDIA','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![instrument_id],
    )
    .unwrap();
    // USD -> CNY 汇率 7.2
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,'USD','CNY',7.2,'2026-06-01','manual','2026-06-01T00:00:00Z',1,'test')",
        params!["er-usd-cny"],
    )
    .unwrap();

    let buy_txn_id = "txn-buy-cross";
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',100000,'USD',720000,?2,NULL,NULL,NULL,'买 NVDA','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
        params![buy_txn_id, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,10000,0)",
        params![buy_txn_id, instrument_id],
    )
    .unwrap();
    // lot 成本币种为 USD（标的币种），与账户 CNY 不同
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,10000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params!["lot-cross", account_id, instrument_id, buy_txn_id],
    )
    .unwrap();
    // 最新价 $120 USD
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,12000,'USD','2026-07-07','yahoo','2026-07-07T00:00:00Z','2026-07-07T00:00:00Z',1,'test')",
        params!["mp-cross", instrument_id],
    )
    .unwrap();

    let (cost_basis, cost_ccy, market_value, unrealized_pnl): (i64, String, i64, i64) = conn
        .query_row(
            "SELECT cost_basis_cents, cost_currency_code, market_value_cents, unrealized_pnl_cents \
             FROM v_holdings WHERE id=?1",
            params![format!("{account_id}-{instrument_id}-USD")],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    // cost_basis_cents 保持原 lot 币种（USD），未折算；折算仅用于盈亏计算
    assert_eq!(cost_basis, 100000);
    assert_eq!(cost_ccy, "USD");
    // 市值 = 10 * 12000 * 7.2 = 864000 CNY 分
    assert_eq!(market_value, 864000);
    // 盈亏 = 864000 - (100000 * 7.2 = 720000) = 144000 CNY 分
    // 旧实现错误地得 864000 - 100000 = 764000（CNY 市值减 USD 成本）
    assert_eq!(unrealized_pnl, 144000);
}

/// v_holdings 反向汇率兌底：CNY 账户持 USD 标的，库里只录了 CNY->USD（反向），
/// 视图应取倒数折算，市值与盈亏与正向 USD->CNY 等价。
#[test]
fn holding_reverse_rate_fallback() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    let account_id = "acc-test-rev";
    let instrument_id = "inst-test-rev";
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'反向','investment','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,'REV','stock','reverse test','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![instrument_id],
    )
    .unwrap();
    // 只录反向汇率 CNY->USD = 0.125（即 1 USD = 8 CNY）
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,'CNY','USD',0.125,'2026-06-01','manual','2026-06-01T00:00:00Z',1,'test')",
        params!["er-rev"],
    )
    .unwrap();

    let buy_txn_id = "txn-buy-rev";
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',80000,'USD',640000,?2,NULL,NULL,NULL,'buy','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
        params![buy_txn_id, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,8000,0)",
        params![buy_txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,8000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params!["lot-rev", account_id, instrument_id, buy_txn_id],
    )
    .unwrap();
    // 最新价 $96 USD
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,9600,'USD','2026-07-07','yahoo','2026-07-07T00:00:00Z','2026-07-07T00:00:00Z',1,'test')",
        params!["mp-rev", instrument_id],
    )
    .unwrap();

    // 正向 USD->CNY 不存在，只能走反向 CNY->USD=0.125 取倒数得 8。
    // 市值 = 10 * 9600 / 0.125 = 768000 CNY 分
    // 成本 = 80000 / 0.125 = 640000 CNY 分
    // 盈亏 = 768000 - 640000 = 128000 CNY 分
    let (market_value, unrealized_pnl): (i64, i64) = conn
        .query_row(
            "SELECT market_value_cents, unrealized_pnl_cents FROM v_holdings WHERE id=?1",
            params![format!("{account_id}-{instrument_id}-USD")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(market_value, 768000);
    assert_eq!(unrealized_pnl, 128000);
}

/// v_holdings 的 id 在同账户同标的但 lot 币种不同时仍保持唯一：
/// id 纳入 currency_code，避免 account_id-instrument_id 重复 key。
#[test]
fn holding_id_unique_across_currencies() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    let account_id = "acc-test-id-uniq";
    let instrument_id = "inst-test-multi";
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'多币种','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,'MULTI','stock','多币种标的','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![instrument_id],
    )
    .unwrap();

    // 同账户同标的两笔买入，但 lot 币种不同（USD 与 EUR），绕过应用层直接写入。
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',10000,'USD',10000,?2,NULL,NULL,NULL,'usd lot','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
        params!["txn-usd", account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',5,2000,0)",
        params!["txn-usd", instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,5,5,2000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params!["lot-usd", account_id, instrument_id, "txn-usd"],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',9000,'EUR',9000,?2,NULL,NULL,NULL,'eur lot','2026-01-11','2026-01-11T00:00:00Z','2026-01-11T00:00:00Z',1,'test',0)",
        params!["txn-eur", account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',3,3000,0)",
        params!["txn-eur", instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,3,3,3000,'EUR','2026-01-11T00:00:00Z','2026-01-11T00:00:00Z',1,'test')",
        params!["lot-eur", account_id, instrument_id, "txn-eur"],
    )
    .unwrap();

    // 应出现两行，id 不同（含 currency_code），无重复。
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT id, cost_currency_code FROM v_holdings ORDER BY cost_currency_code")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 2, "应按 lot 币种拆成两行");
    assert_eq!(rows[0].0, format!("{account_id}-{instrument_id}-EUR"));
    assert_eq!(rows[0].1, "EUR");
    assert_eq!(rows[1].0, format!("{account_id}-{instrument_id}-USD"));
    assert_eq!(rows[1].1, "USD");

    // id 在视图内全局唯一。
    let dupes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT id FROM v_holdings GROUP BY id HAVING COUNT(*) > 1)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dupes, 0, "id 不应重复");
}

/// v_holdings 过滤软删除账户：已删账户的 lot 仍存在于 security_lots，但视图不应返回其持仓行。
#[test]
fn holding_excludes_soft_deleted_account() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    let active_acc = "acc-soft-active";
    let deleted_acc = "acc-soft-deleted";
    let instrument_id = "inst-soft-test";
    for acc in [active_acc, deleted_acc] {
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,'investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![acc, acc],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,'SOFT','stock','soft-delete test','USD','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![instrument_id],
    )
    .unwrap();

    // 两个账户各一笔买入 + lot
    for (acc, txn, lot) in [
        (active_acc, "txn-soft-a", "lot-soft-a"),
        (deleted_acc, "txn-soft-d", "lot-soft-d"),
    ] {
        conn.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'buy',10000,'USD',10000,?2,NULL,NULL,NULL,'buy','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
            params![txn, acc],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
             VALUES (?1,?2,'buy',1,10000,0)",
            params![txn, instrument_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,1,1,10000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
            params![lot, acc, instrument_id, txn],
        )
        .unwrap();
    }

    // 软删除 deleted_acc（UPDATE is_deleted=1，lot 仍存在）
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at='2026-02-01T00:00:00Z', version=version+1 WHERE id=?1",
        params![deleted_acc],
    )
    .unwrap();

    // 已删账户的 lot 仍在 security_lots，证明过滤发生在视图层而非数据被删。
    let lot_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE account_id=?1 AND remaining_quantity > 0",
            params![deleted_acc],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lot_count, 1, "已删账户的 lot 应仍存在");

    // 视图应只剩 active 账户的持仓行。
    let account_ids: Vec<String> = conn
        .prepare("SELECT account_id FROM v_holdings ORDER BY account_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(
        account_ids,
        vec![active_acc.to_string()],
        "软删除账户的持仓不应出现在视图"
    );
}

/// security_lots 聚合索引：partial covering index 存在并覆盖聚合列，旧冗余索引已删除，
/// 且 v_holdings 聚合子查询实际命中该覆盖索引（EXPLAIN QUERY PLAN 出现索引名）。
#[test]
fn security_lots_active_covering_index() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    // 新 partial covering index 存在，含 partial 谓词与全部聚合列。
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_security_lots_active_covering'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        sql.contains("remaining_quantity > 0"),
        "应为 partial index: {sql}"
    );
    for col in [
        "account_id",
        "instrument_id",
        "currency_code",
        "remaining_quantity",
        "cost_per_unit_cents",
        "updated_at",
    ] {
        assert!(sql.contains(col), "covering index 应包含 {col}: {sql}");
    }

    // 旧冗余索引已删除（account_id+instrument_id 查询由 UNIQUE 自动索引覆盖）。
    let old: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_security_lots_account_instrument'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        old, 0,
        "旧冗余索引 idx_security_lots_account_instrument 应已删除"
    );

    // 聚合子查询应使用新覆盖索引，避免全表扫描与回表。
    let mut stmt = conn
        .prepare(
            "EXPLAIN QUERY PLAN \
             SELECT account_id, instrument_id, currency_code, \
             SUM(remaining_quantity), SUM(remaining_quantity * cost_per_unit_cents), MAX(updated_at) \
             FROM security_lots WHERE remaining_quantity > 0 \
             GROUP BY account_id, instrument_id, currency_code",
        )
        .unwrap();
    let details: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    let plan = details.join(" | ");
    assert!(
        plan.contains("idx_security_lots_active_covering"),
        "聚合应使用 idx_security_lots_active_covering: {plan}"
    );
}

/// 非本位币交易按日期汇率折算到 amount_native_cents。
#[test]
fn transaction_currency_conversion() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    let account_id = "acc-test-cny";
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'现金','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![account_id],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,'USD','CNY',7.2,'2026-01-01','manual','2026-01-01T00:00:00Z',1,'test')",
        params!["er-01"],
    )
    .unwrap();

    let native = crate::transaction::amount::convert_to_native(&conn, 10000, "USD").unwrap();
    assert_eq!(native, 72000);

    // 同币种无需汇率，1:1 返回。
    let native = crate::transaction::amount::convert_to_native(&conn, 10000, "CNY").unwrap();
    assert_eq!(native, 10000);
}

// ---------------------------------------------------------------------------
// Perf trace（数据库耗时日志）测试——ADR-0009
// ---------------------------------------------------------------------------

/// 时序级别纯函数边界：0、恰好阈值、略低于/略高于阈值、阈值 0。
#[test]
fn timing_level_boundaries() {
    use perf_trace::TimingClass;

    let threshold = Duration::from_millis(100);

    // 0 耗时：远低于阈值 → 正常（debug 明细）。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::ZERO),
        TimingClass::Normal
    );
    // 恰好等于阈值 → 正常（边界语义为严格大于才升级慢查询）。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(100)),
        TimingClass::Normal
    );
    // 略低于阈值 → 正常。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(99)),
        TimingClass::Normal
    );
    // 略高于阈值 → 慢查询（warn）。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(101)),
        TimingClass::Slow
    );
    // threshold=0 且 duration>0 → 慢查询（0 阈值下非零耗时即慢查询）。
    assert_eq!(
        perf_trace::timing_level(Duration::ZERO, Duration::from_nanos(1)),
        TimingClass::Slow
    );
}

/// 接线回归：open_in_memory 默认注册 hook，执行 SELECT 1 能捕获到含 SQL 文本的事件。
/// 不限定具体级别——级别分类由 `timing_level` 纯函数测试覆盖；此处只验证 hook 接线生效
/// 且事件带 SQL 原文（占位符 SQL 记录于所有级别）。
#[test]
fn perf_trace_factory_emits_sql_event() {
    let conn = open_in_memory().unwrap();

    let events = capture_events(|| {
        conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .unwrap();
    });

    assert!(
        events.iter().any(|e| e
            .fields
            .iter()
            .any(|(k, v)| k == "sql" && v.contains("SELECT 1"))),
        "应捕获到含 SQL 文本的事件，实际捕获: {events:?}"
    );
}

/// 接线回归：threshold=0 时无需构造慢语句，正常语句也命中 warn 分支。
/// （SELECT 1 在内存库中耗时可能为 0ns，`0 > 0` 仍为 false；故用递归 CTE
/// 保证一条真实耗时的语句，验证阈值注入生效。）
#[test]
fn perf_trace_zero_threshold_emits_warn() {
    let conn = Connection::open_in_memory().unwrap();
    perf_trace::install_perf_trace(&conn, Duration::ZERO);

    let events = capture_events(|| {
        conn.query_row(
            "SELECT SUM(n) FROM (\
             WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM s WHERE n < 200000)\n             SELECT n FROM s)",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    });

    assert!(
        events.iter().any(|e| e.level == Level::WARN),
        "threshold=0 时正常语句也应命中 warn 分支"
    );
}

/// 接线回归：在 `command` span 内执行 SQL，SQL 耗时事件应归因到该 span
/// （当前 span 名为 `command`）。这验证了 IPC 侧 `logged_invoke_handler`
/// 用 `info_span!(command, id_hint)` 包裹命令执行后，hook 事件自动继承调用方 span
/// （同步命令与 wrapper 同线程执行，归因成立）。
#[test]
fn perf_trace_sql_event_inherits_command_span() {
    let conn = open_in_memory().unwrap();

    let events = capture_events(|| {
        // 与 `logged_invoke_handler` 一致的命令 span 形状：name=command，含 command 字段。
        let span = tracing::info_span!("command", command = "list_accounts", id_hint = "");
        let _entered = span.enter();
        conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .unwrap();
    });

    let sql_events: Vec<_> = events
        .iter()
        .filter(|e| e.fields.iter().any(|(k, _)| k == "sql"))
        .collect();
    assert!(
        !sql_events.is_empty(),
        "应捕获到 SQL 事件，实际捕获: {events:?}"
    );
    assert!(
        sql_events
            .iter()
            .all(|e| e.current_span.as_deref() == Some("command")),
        "SQL 事件应归因到 command span，实际: {sql_events:?}"
    );
}

// ---------------------------------------------------------------------------
// V010：价格历史化（issue #136 / ADR-0019）——price_history 与 fx_rate_history
// ---------------------------------------------------------------------------

/// 在测试库中创建一个可引用的金融工具（依赖 init_db 种子币种）。
fn insert_instrument(conn: &Connection, id: &str, currency: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'600519.SH','stock','贵州茅台',?2,'sh','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, currency],
    )
    .unwrap();
}

/// 写入一条 price_history（周采样价格历史）。
fn insert_price_history(conn: &Connection, id: &str, instrument_id: &str, trade_date: &str) {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,170000,'CNY','eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        params![id, instrument_id, trade_date],
    )
    .unwrap();
}

/// 写入一条 fx_rate_history（周采样汇率历史）。
fn insert_fx_rate_history(
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

/// 旧版本发布备份停驻的 schema 版本：发布 tag 时的迁移序列长度（现序列为 9 个），
/// 恢复旧备份即停在此版本，由 init_db 补齐后续迁移。旧备份可能缺
/// app_settings（位置语义重排）：读侧 settings::get 缺表返回默认值、
/// 写侧 settings::set 就地建表自愈。
const V030_SCHEMA_VERSION: usize = 7;

/// price_history：周采样唯一约束（每标的每周至多一条，同周不同采样日也拒绝）
/// + 标的级联删除跟随。
#[test]
fn price_history_weekly_unique_and_cascade() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    insert_instrument(&conn, "inst-01", "CNY");

    insert_price_history(&conn, "ph-01", "inst-01", "2026-05-27");
    // 同标的同采样日第二行应被周唯一约束拒绝（整周覆盖走 upsert，不产生重复）。
    let dup = conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES ('ph-02','inst-01','2026-05-27',170000,'CNY','eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        [],
    );
    assert!(dup.is_err(), "同标的同采样日第二行应违反周唯一约束");
    // 同周不同采样日（周三 vs 周五）同样应被拒绝——「每周至多一条」由库层强制。
    let dup_same_week = conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES ('ph-02b','inst-01','2026-05-29',171000,'CNY','eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        [],
    );
    assert!(
        dup_same_week.is_err(),
        "同周不同采样日第二行应违反周唯一约束"
    );
    // 不同周（另一采样周）可正常写入。
    insert_price_history(&conn, "ph-03", "inst-01", "2026-06-03");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);

    // 标的删除 → 历史级联删除跟随。
    conn.execute("DELETE FROM instruments WHERE id='inst-01'", [])
        .unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "删除标的后价格历史应级联删除");
}

/// fx_rate_history：币种对 × 周唯一（与 PriceHistory 同规则）。
#[test]
fn fx_rate_history_weekly_unique_per_pair() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    insert_fx_rate_history(&conn, "fx-01", "HKD", "CNY", "2026-05-27", 0.92);
    // 同币种对同采样日第二行应被周唯一约束拒绝。
    let dup = conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES ('fx-02','HKD','CNY','2026-05-27',0.92,'eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        [],
    );
    assert!(dup.is_err(), "同币种对同采样日第二行应违反周唯一约束");
    // 同周不同采样日同样拒绝——周采样语义与 PriceHistory 对齐。
    let dup_same_week = conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES ('fx-02b','HKD','CNY','2026-05-29',0.93,'eastmoney','2026-06-01T00:00:00Z','2026-06-01T00:00:00Z',1,'test')",
        [],
    );
    assert!(dup_same_week.is_err(), "同币种对同周第二行应违反周唯一约束");
    // 不同周可写入；反向币种对是另一条序列，互不冲突。
    insert_fx_rate_history(&conn, "fx-03", "HKD", "CNY", "2026-06-03", 0.92);
    insert_fx_rate_history(&conn, "fx-04", "CNY", "HKD", "2026-05-27", 1.087);
}

/// 旧版本备份恢复后升级路径：旧库停在发布时的 schema 版本，经 init_db 补齐
/// 后续迁移，price_history / fx_rate_history 自动创建。
#[test]
fn migration_upgrades_v030_backup_with_new_tables() {
    let mut conn = open_in_memory().unwrap();
    migrations()
        .to_version(&mut conn, V030_SCHEMA_VERSION)
        .unwrap();
    let before: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, V030_SCHEMA_VERSION as i64);

    // 旧库中已有数据（如一个账户）在升级后应原样保留。
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,is_deleted,created_at,updated_at,version,device_id) \
         VALUES ('acc-01','现金','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();

    init_db(&mut conn).unwrap();

    // 新表存在且可直接写入（迁移不止是建表语句语法有效，约束也生效）。
    insert_instrument(&conn, "inst-up", "CNY");
    insert_price_history(&conn, "ph-up", "inst-up", "2026-05-27");
    insert_fx_rate_history(&conn, "fx-up", "HKD", "CNY", "2026-05-27", 0.92);

    // 旧数据未受迁移影响。
    let acc: String = conn
        .query_row("SELECT name FROM accounts WHERE id='acc-01'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(acc, "现金");
}

// ---------------------------------------------------------------------------
// 连接层统一写入口 db::write（ADR-0032）
// ---------------------------------------------------------------------------

/// 构造带 Arc<Mutex<Connection>> 的 DbState（写入口持锁形态）。
fn write_test_state() -> DbState {
    DbState::open_in_memory().expect("打开内存库")
}

/// 读回自动备份调度状态（断言置脏语义用）。
fn dirty_state(state: &DbState) -> crate::auto_backup::AutoBackupState {
    let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
    crate::auto_backup::get_state(&conn).expect("读调度状态")
}

/// 闭包成功且已提交（autocommit）→ 单点置脏；目录未配置时到期检查静默跳过
/// （不记备份锚点）。
#[test]
fn write_ok_marks_dirty() {
    let state = write_test_state();
    assert!(!dirty_state(&state).dirty, "初始应为洁");
    state.write(|_conn| Ok(())).expect("写入口成功");
    assert!(dirty_state(&state).dirty, "闭包成功后应置脏");
    assert_eq!(
        dirty_state(&state).last_backup_at,
        None,
        "目录未配置不应记录备份锚点"
    );
}

/// 闭包失败 → 不置脏（回滚语义：失败闭包不该留下置脏痕迹）。
#[test]
fn write_err_does_not_mark_dirty() {
    let state = write_test_state();
    let err = state
        .write(|_conn| Err::<(), AppError>(AppError::Invalid("boom".into())))
        .unwrap_err();
    assert!(err.to_string().contains("boom"));
    assert!(!dirty_state(&state).dirty, "闭包失败不应置脏");
}

/// 闭包内部自行 BEGIN 且未提交就返回 Ok → is_autocommit 为假，写入口不在
/// 未提交点置脏；回滚后既无数据也无置脏（提交点语义：置脏只发生在提交点）。
#[test]
fn write_inside_open_transaction_defers_to_commit_point() {
    let state = write_test_state();
    state
        .write(|conn| {
            conn.execute("BEGIN", [])?;
            // 任意一笔真实写（未提交）：用调度状态 KV，避开业务表外键。
            crate::settings::set(
                conn,
                crate::settings::SettingKey::AutoBackupNextDueAt,
                &Some(String::from("2026-01-01T00:00:00Z")),
            )?;
            Ok(())
        })
        .expect("闭包成功");
    assert!(!dirty_state(&state).dirty, "未提交不置脏");
    {
        let conn = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute("ROLLBACK", []).expect("回滚");
        let hit: Option<String> = crate::settings::get(
            &conn,
            crate::settings::SettingKey::AutoBackupNextDueAt,
            None,
        )
        .unwrap();
        assert_eq!(hit, None, "回滚后写入应消失");
    }
    assert!(!dirty_state(&state).dirty, "回滚后仍不置脏");
}

/// 闭包内部自行 BEGIN/COMMIT 后返回 Ok → 已回到提交点（is_autocommit），
/// 写入口在该点单点置脏（交易修改路径的形态）。
#[test]
fn write_closure_committing_own_tx_marks_dirty() {
    let state = write_test_state();
    state
        .write(|conn| {
            conn.execute("BEGIN", [])?;
            crate::settings::set(
                conn,
                crate::settings::SettingKey::AutoBackupNextDueAt,
                &Some(String::from("2026-01-01T00:00:00Z")),
            )?;
            conn.execute("COMMIT", [])?;
            Ok(())
        })
        .expect("闭包成功");
    assert!(dirty_state(&state).dirty, "提交点应置脏");
}
