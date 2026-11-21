use std::time::Duration;

use rusqlite::Connection;
use tracing::Level;

use crate::db::perf_trace;
use crate::test_utils::capture_events;
use tauri_app_lib::test_support::{self, seed_account, seed_instrument};

#[test]
fn security_lots_active_covering_index() {
    let conn = test_support::open();

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

#[test]
fn timing_level_boundaries() {
    use perf_trace::TimingClass;

    let threshold = Duration::from_millis(100);

    assert_eq!(
        perf_trace::timing_level(threshold, Duration::ZERO),
        TimingClass::Normal
    );
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(100)),
        TimingClass::Normal
    );
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(99)),
        TimingClass::Normal
    );
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(101)),
        TimingClass::Slow
    );
    assert_eq!(
        perf_trace::timing_level(Duration::ZERO, Duration::from_nanos(1)),
        TimingClass::Slow
    );
}

#[test]
fn perf_trace_factory_emits_sql_event() {
    let conn = test_support::open();

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

#[test]
fn perf_trace_zero_threshold_emits_warn() {
    let conn = test_support::open();
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

fn v016_plan<P>(conn: &Connection, sql: &str, params: P) -> String
where
    P: rusqlite::Params,
{
    let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let details: Vec<String> = stmt
        .query_map(params, |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    details.join(" | ")
}

fn v016_world() -> Connection {
    let conn = test_support::open();

    seed_account(&conn, "acc-01", "现金", "cash", "CNY", 0);
    seed_account(&conn, "acc-02", "储蓄卡", "bank", "CNY", 0);
    seed_instrument(&conn, "inst-01", "600000.SH", "浦发银行", "CNY", "sh");
    conn.execute(
        "INSERT INTO categories (id,name,kind,created_at,updated_at,version,device_id) \
         VALUES ('cat-01','餐饮','expense',?1,?1,1,'test')",
        rusqlite::params![test_support::FIXED_NOW],
    )
    .unwrap();
    conn.execute_batch(
        "WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM seq WHERE i<2000)\
         INSERT INTO transactions\
           (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
            category_id,merchant_id,refund_of_transaction_id,note,dedup_hash,date,created_at,\
            updated_at,version,device_id,is_deleted)\
         SELECT printf('tx-%04d', i), 'expense', 100, 'CNY', 100,\
                CASE WHEN i%2=0 THEN 'acc-01' ELSE 'acc-02' END,\
                NULL, CASE WHEN i%3=0 THEN 'cat-01' ELSE NULL END, \
                NULL, NULL, NULL, NULL,\
                date('2026-03-01', '-' || (i%90) || ' days'),\
                '2026-03-01T08:00:00Z', '2026-03-01T08:00:00Z',\
                1, 'test', CASE WHEN i%100=0 THEN 1 ELSE 0 END \
         FROM seq;\
         INSERT INTO transactions\
           (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,\
            updated_at,version,device_id,is_deleted)\
         VALUES ('tx-buy-01','buy',10000,'CNY',10000,'acc-01','2026-03-01',\
                 '2026-03-01T08:00:00Z','2026-03-01T08:00:00Z',1,'test',0);\
         INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES ('tx-buy-01','inst-01','buy',100.0,10000,0);\
         ANALYZE;",
    )
    .unwrap();
    conn
}

#[test]
fn v016_structural_indexes_exist_with_covering_columns() {
    let conn = test_support::open();

    let expected: &[(&str, &[&str])] = &[
        ("idx_transactions_list_order", &["date", "created_at", "id"]),
        (
            "idx_transactions_account_date",
            &["account_id", "date", "created_at", "id"],
        ),
        (
            "idx_transactions_account_flow",
            &["account_id", "kind", "amount_native_cents"],
        ),
        (
            "idx_transactions_to_account_flow",
            &["to_account_id", "kind", "amount_native_cents"],
        ),
        (
            "idx_transactions_month_expr",
            &["substr(date, 1, 7)", "kind", "amount_native_cents", "date"],
        ),
        (
            "idx_transactions_category_covering",
            &["category_id", "kind", "date", "amount_native_cents"],
        ),
    ];
    for (name, columns) in expected {
        let sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='index' AND name=?1",
                [*name],
                |r| r.get(0),
            )
            .unwrap_or_else(|_| panic!("索引 {name} 应存在"));
        assert!(
            sql.contains("is_deleted = 0"),
            "{name} 应为 partial index: {sql}"
        );
        for col in *columns {
            assert!(sql.contains(col), "{name} 应包含 {col}: {sql}");
        }
    }
}

#[test]
fn v016_list_pagination_uses_list_order_index_without_temp_btree() {
    let conn = v016_world();
    let sql = "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
               to_account_id,category_id,refund_of_transaction_id,note,date,created_at,\
               updated_at,version,device_id,is_deleted,merchant_id,policy_id,fx_rate_used,fx_rate_source \
               FROM transactions WHERE is_deleted=0 \
               ORDER BY date DESC, created_at DESC, id DESC";

    for (label, tail) in [
        ("首页", " LIMIT 20 OFFSET 0"),
        ("深分页", " LIMIT 20 OFFSET 1980"),
    ] {
        let plan = v016_plan(&conn, &format!("{sql}{tail}"), []);
        assert!(
            plan.contains("idx_transactions_list_order"),
            "{label}应由列表序索引驱动: {plan}"
        );
        assert!(
            !plan.contains("TEMP B-TREE"),
            "{label}不应有 ORDER BY 临时 B-tree: {plan}"
        );
    }
}

#[test]
fn v016_account_date_filter_uses_account_date_index() {
    let conn = v016_world();
    let plan = v016_plan(
        &conn,
        "SELECT id,kind,amount_cents FROM transactions \
         WHERE is_deleted=0 AND date>='2026-02-01' AND date<='2026-03-01' \
         AND account_id='acc-01' \
         ORDER BY date DESC, created_at DESC, id DESC LIMIT 20 OFFSET 0",
        [],
    );
    assert!(
        plan.contains("idx_transactions_account_date"),
        "账户日期筛选应由账户筛选序索引驱动: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE"),
        "账户日期筛选不应有 ORDER BY 临时 B-tree: {plan}"
    );
}

#[test]
fn v016_balance_flow_aggregates_use_cash_flow_indexes() {
    let conn = v016_world();
    let out_plan = v016_plan(
        &conn,
        "SELECT COALESCE(SUM(CASE t.kind \
         WHEN 'income' THEN t.amount_native_cents WHEN 'expense' THEN -t.amount_native_cents \
         ELSE 0 END),0) FROM transactions t \
         WHERE t.is_deleted=0 AND t.account_id='acc-01'",
        [],
    );
    assert!(
        out_plan.contains("idx_transactions_account_flow"),
        "转出侧聚合应由账户现金流索引驱动: {out_plan}"
    );
    let in_plan = v016_plan(
        &conn,
        "SELECT COALESCE(SUM(CASE t.kind \
         WHEN 'income' THEN t.amount_native_cents WHEN 'expense' THEN -t.amount_native_cents \
         ELSE 0 END),0) FROM transactions t \
         WHERE t.is_deleted=0 AND t.to_account_id='acc-01'",
        [],
    );
    assert!(
        in_plan.contains("idx_transactions_to_account_flow"),
        "转入侧聚合应由转入现金流索引驱动: {in_plan}"
    );
}

#[test]
fn v016_monthly_summary_uses_pinned_expression_index() {
    let conn = v016_world();
    let period_plan = v016_plan(
        &conn,
        "SELECT substr(date,1,7) AS month, \
         SUM(CASE WHEN kind IN ('income','dividend') THEN amount_native_cents ELSE 0 END), \
         SUM(CASE WHEN kind='expense' THEN amount_native_cents \
                  WHEN kind='refund' THEN -amount_native_cents ELSE 0 END) \
         FROM transactions INDEXED BY idx_transactions_month_expr \
         WHERE is_deleted=0 AND date>='2026-01-01' AND date<='2026-03-01' \
         GROUP BY month ORDER BY month",
        [],
    );
    assert!(
        period_plan.contains("idx_transactions_month_expr"),
        "月度汇总（期间口径）应由表达式索引驱动: {period_plan}"
    );
    assert!(
        !period_plan.contains("TEMP B-TREE"),
        "月度汇总（期间口径）不应有 GROUP BY 临时 B-tree: {period_plan}"
    );

    let year_plan = v016_plan(
        &conn,
        "SELECT substr(date,1,7) AS month, \
         SUM(CASE WHEN kind IN ('income','dividend') THEN amount_native_cents ELSE 0 END) \
         FROM transactions INDEXED BY idx_transactions_month_expr \
         WHERE is_deleted=0 AND substr(date,1,4)='2026' \
         GROUP BY month ORDER BY month",
        [],
    );
    assert!(
        year_plan.contains("idx_transactions_month_expr"),
        "月度汇总（年份口径）应由表达式索引驱动: {year_plan}"
    );
    assert!(
        !year_plan.contains("TEMP B-TREE"),
        "月度汇总（年份口径）不应有 GROUP BY 临时 B-tree: {year_plan}"
    );
}

#[test]
fn v016_category_shares_use_category_covering_index() {
    let conn = v016_world();
    let plan = v016_plan(
        &conn,
        "SELECT t.category_id, SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
         WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END) \
         FROM transactions t INDEXED BY idx_transactions_category_covering \
         LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 \
         GROUP BY t.category_id ORDER BY 2 DESC",
        [],
    );
    assert!(
        plan.contains("idx_transactions_category_covering"),
        "分类聚合应由分类覆盖索引驱动: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE FOR GROUP BY"),
        "分类聚合不应有 GROUP BY 临时 B-tree（覆盖索引自带分组序）: {plan}"
    );
}

#[test]
fn v016_category_shares_period_form_stays_on_category_covering_index() {
    let conn = v016_world();
    let plan = v016_plan(
        &conn,
        "SELECT t.category_id, SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
         WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END) \
         FROM transactions t INDEXED BY idx_transactions_category_covering \
         LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 \
         AND t.date>='2026-01-01' AND t.date<='2026-12-31' \
         GROUP BY t.category_id ORDER BY 2 DESC",
        [],
    );
    assert!(
        plan.contains("idx_transactions_category_covering"),
        "期间口径分类聚合应由分类覆盖索引驱动: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE FOR GROUP BY"),
        "期间口径分类聚合不应有 GROUP BY 临时 B-tree: {plan}"
    );
}

fn v030_merchant_world() -> Connection {
    let conn = v016_world();
    conn.execute_batch(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('m-01','商户一','2026-03-01T08:00:00Z','2026-03-01T08:00:00Z',1,'test',0);
         UPDATE transactions SET merchant_id='m-01' \
         WHERE is_deleted=0 AND CAST(substr(id,4) AS INTEGER) % 2 = 0;
         ANALYZE;",
    )
    .unwrap();
    conn
}

#[test]
fn v030_merchant_shares_use_merchant_covering_index() {
    let conn = v030_merchant_world();
    let plan = v016_plan(
        &conn,
        "SELECT t.merchant_id, m.name, SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
         WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END), COUNT(*) \
         FROM transactions t INDEXED BY idx_transactions_merchant_covering \
         JOIN merchants m ON m.id=t.merchant_id \
         WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 \
         AND substr(t.date,1,4)='2026' \
         GROUP BY t.merchant_id ORDER BY 3 DESC, m.name",
        [],
    );
    assert!(
        plan.contains("idx_transactions_merchant_covering"),
        "商户占比应由商户覆盖索引驱动: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE FOR GROUP BY"),
        "商户占比不应有 GROUP BY 临时 B-tree（覆盖索引自带分组序）: {plan}"
    );
}

#[test]
fn v030_merchant_shares_period_form_stays_on_merchant_covering_index() {
    let conn = v030_merchant_world();
    let plan = v016_plan(
        &conn,
        "SELECT t.merchant_id, m.name, SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
         WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END), COUNT(*) \
         FROM transactions t INDEXED BY idx_transactions_merchant_covering \
         JOIN merchants m ON m.id=t.merchant_id \
         WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 \
         AND t.date>='2026-01-01' AND t.date<='2026-12-31' \
         GROUP BY t.merchant_id ORDER BY 3 DESC, m.name",
        [],
    );
    assert!(
        plan.contains("idx_transactions_merchant_covering"),
        "期间口径商户占比应由商户覆盖索引驱动: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE FOR GROUP BY"),
        "期间口径商户占比不应有 GROUP BY 临时 B-tree: {plan}"
    );
}

#[test]
fn v016_date_range_probe_uses_index_endpoints() {
    let conn = v016_world();
    let plan = v016_plan(
        &conn,
        "SELECT (SELECT MIN(date) FROM transactions WHERE is_deleted=0), \
         (SELECT MAX(date) FROM transactions WHERE is_deleted=0)",
        [],
    );
    assert!(
        plan.matches("idx_transactions_list_order").count() >= 2,
        "MIN 与 MAX 两个标量子查询都应经列表序索引定位: {plan}"
    );
    assert!(
        !plan.contains("SCAN transactions"),
        "不应存在无索引的全表扫描: {plan}"
    );
}

#[test]
fn v016_holdings_as_of_driven_from_security_transactions() {
    let conn = v016_world();
    let plan = v016_plan(
        &conn,
        "SELECT COALESCE(SUM(\
             CASE security_transactions.action WHEN 'buy' THEN security_transactions.quantity \
                  ELSE -security_transactions.quantity END\
         ), 0.0) \
         FROM security_transactions \
         JOIN transactions t ON t.id = security_transactions.transaction_id \
         JOIN accounts a ON a.id = t.account_id \
         WHERE security_transactions.action IN ('buy','sell') \
           AND security_transactions.quantity IS NOT NULL \
           AND t.is_deleted = 0 \
           AND a.is_deleted = 0 \
           AND security_transactions.instrument_id = COALESCE(?2, security_transactions.instrument_id) \
           AND t.date <= ?1",
        rusqlite::params!["2026-03-01", Option::<&str>::None],
    );
    let first_line = plan.split(" | ").next().unwrap_or_default();
    assert!(
        first_line.contains("security_transactions"),
        "时点持仓应从 security_transactions 侧驱动: {plan}"
    );
}

#[test]
fn v001_dedup_hash_partial_index_exists() {
    let conn = test_support::open();

    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_transactions_dedup_hash'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("索引 idx_transactions_dedup_hash 应存在"));
    assert!(
        sql.contains("is_deleted = 0") && sql.contains("dedup_hash IS NOT NULL"),
        "应为双条件 partial index: {sql}"
    );
    for col in ["dedup_hash", "created_at"] {
        assert!(sql.contains(col), "索引应包含 {col}: {sql}");
    }
}

#[test]
fn v001_dedup_fallback_query_uses_dedup_hash_index_without_temp_btree() {
    let conn = v016_world();
    conn.execute_batch(
        "UPDATE transactions SET dedup_hash = 'h-' || id \
         WHERE is_deleted = 0 AND CAST(substr(id, 4) AS INTEGER) % 3 = 0;\
         ANALYZE;",
    )
    .unwrap();

    let plan = v016_plan(
        &conn,
        "SELECT id FROM transactions \
         WHERE dedup_hash=?1 AND is_deleted=0 ORDER BY created_at LIMIT 1",
        rusqlite::params!["h-tx-0003"],
    );
    assert!(
        plan.contains("SEARCH") && !plan.contains("SCAN"),
        "兜底查询应经索引 SEARCH 定位而非 SCAN 全表: {plan}"
    );
    assert!(
        plan.contains("idx_transactions_dedup_hash"),
        "兜底查询应命中 dedup_hash 部分索引: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE"),
        "ORDER BY created_at 应由索引列序满足，无临时 B-tree: {plan}"
    );
}

fn v025_world() -> Connection {
    let conn = v016_world();
    conn.execute_batch(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
          date,created_at,updated_at,version,device_id,is_deleted) VALUES\
          ('tx-tr-01','transfer',500,'CNY',500,'acc-01','acc-02','2026-02-01',\
           '2026-03-01T08:00:00Z','2026-03-01T08:00:00Z',1,'test',0),\
          ('tx-tr-02','transfer',300,'CNY',300,'acc-02','acc-01','2026-02-02',\
           '2026-03-01T08:00:00Z','2026-03-01T08:00:00Z',1,'test',0);\
         UPDATE transactions SET funding_account_id='acc-01' WHERE id='tx-buy-01';\
         ANALYZE;",
    )
    .unwrap();
    conn
}

#[test]
fn v025_funding_flow_aggregate_uses_funding_index() {
    let conn = v025_world();
    let plan = v016_plan(
        &conn,
        "SELECT COALESCE(SUM(CASE t.kind WHEN 'buy' THEN t.amount_native_cents \
         WHEN 'sell' THEN -t.amount_native_cents ELSE 0 END),0) FROM transactions t \
         WHERE t.is_deleted=0 AND t.funding_account_id='acc-01'",
        [],
    );
    assert!(
        plan.contains("idx_transactions_funding"),
        "出资侧聚合应由出资端索引驱动: {plan}"
    );
}

#[test]
fn v025_idempotency_dedup_uses_idempotency_key_index() {
    let conn = v016_world();
    conn.execute_batch(
        "UPDATE transactions SET idempotency_key = 'k-' || id \
         WHERE is_deleted = 0 AND CAST(substr(id, 4) AS INTEGER) % 3 = 0;\
         ANALYZE;",
    )
    .unwrap();

    let plan = v016_plan(
        &conn,
        "SELECT id FROM transactions \
         WHERE idempotency_key=?1 AND is_deleted=0 LIMIT 1",
        rusqlite::params!["k-tx-0003"],
    );
    assert!(
        plan.contains("SEARCH") && !plan.contains("SCAN transactions"),
        "幂等键查重应经索引 SEARCH 定位而非 SCAN 全表: {plan}"
    );
    assert!(
        plan.contains("idx_transactions_idempotency_key"),
        "幂等键查重应命中幂等键部分唯一索引: {plan}"
    );
}

#[test]
fn v025_merchant_filter_list_pinned_to_merchant_index() {
    let conn = v025_world();
    let plan = v016_plan(
        &conn,
        "SELECT id FROM transactions INDEXED BY idx_transactions_merchant \
         WHERE is_deleted=0 AND merchant_id='m-01' \
         ORDER BY date DESC, created_at DESC, id DESC LIMIT 20 OFFSET 0",
        [],
    );
    assert!(
        plan.contains("SEARCH") && plan.contains("idx_transactions_merchant"),
        "商户筛选应由商户索引等值定位: {plan}"
    );
}

#[test]
fn v025_date_extremes_pinned_to_date_index() {
    let conn = v025_world();
    let plan = v016_plan(
        &conn,
        "SELECT (SELECT MIN(date) FROM transactions INDEXED BY idx_transactions_date \
         WHERE is_deleted=0), \
         (SELECT MAX(date) FROM transactions INDEXED BY idx_transactions_date \
         WHERE is_deleted=0)",
        [],
    );
    assert!(
        plan.matches("idx_transactions_date").count() >= 2,
        "MIN 与 MAX 两个标量子查询都应由日期索引定位: {plan}"
    );
}

#[test]
fn v025_note_search_stage1_scans_note_search_index() {
    let conn = v025_world();
    let plan = v016_plan(
        &conn,
        "SELECT t.id FROM transactions t INDEXED BY idx_transactions_note_search \
         WHERE t.is_deleted = 0 \
         AND (t.note LIKE '%x%' ESCAPE '\\' OR t.account_id IN ('acc-01') \
              OR t.merchant_id IN ('m-01')) \
         ORDER BY t.date DESC, t.created_at DESC, t.id DESC",
        [],
    );
    assert!(
        plan.contains("SCAN") && plan.contains("idx_transactions_note_search"),
        "搜索第一段应流式扫描搜索覆盖索引: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE"),
        "排序应由索引列序满足，无临时 B-tree: {plan}"
    );
}

#[test]
fn v025_category_filter_list_pinned_to_covering_index() {
    let conn = v025_world();
    let plan = v016_plan(
        &conn,
        "SELECT id FROM transactions INDEXED BY idx_transactions_category_covering \
         WHERE is_deleted=0 AND category_id='cat-01' \
         ORDER BY date DESC, created_at DESC, id DESC LIMIT 20 OFFSET 0",
        [],
    );
    assert!(
        plan.contains("SEARCH") && plan.contains("idx_transactions_category_covering"),
        "分类筛选应由分类覆盖索引等值定位: {plan}"
    );
}

#[test]
fn v025_account_reference_guard_uses_flow_indexes() {
    let conn = v025_world();
    let plan = v016_plan(
        &conn,
        "SELECT CASE WHEN \
         EXISTS(SELECT 1 FROM transactions WHERE account_id=?1 AND is_deleted=0) \
         OR EXISTS(SELECT 1 FROM transactions WHERE to_account_id=?1 AND is_deleted=0) \
         THEN 1 ELSE 0 END",
        rusqlite::params!["acc-01"],
    );
    assert!(
        plan.contains("idx_transactions_account_flow"),
        "守卫转出侧应由转出现金流索引驱动: {plan}"
    );
    assert!(
        plan.contains("idx_transactions_to_account_flow"),
        "守卫转入侧应由转入现金流索引驱动: {plan}"
    );
    assert!(
        !plan.contains("SCAN transactions"),
        "守卫不应存在无索引的全表扫描: {plan}"
    );
}

#[test]
fn v025_policy_stats_uses_policy_covering_index() {
    let conn = v025_world();
    conn.execute_batch(
        "INSERT INTO policies \
         (id,insurer_id,policy_number,product_name,start_date,created_at,updated_at,version,device_id,is_deleted) \
         SELECT 'pol-01', id, 'P-001', '测试险种', '2020-01-01', \
                '2026-03-01T08:00:00Z','2026-03-01T08:00:00Z',1,'test',0 \
         FROM insurers LIMIT 1; \
         INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,category_id,policy_id,\
          date,created_at,updated_at,version,device_id,is_deleted) VALUES\
          ('tx-pol-01','expense',1000,'CNY',1000,'acc-01','cat-01','pol-01','2026-02-10',\
           '2026-03-01T08:00:00Z','2026-03-01T08:00:00Z',1,'test',0),\
          ('tx-pol-02','expense',1000,'CNY',1000,'acc-01','cat-01','pol-01','2026-02-11',\
           '2026-03-01T08:00:00Z','2026-03-01T08:00:00Z',1,'test',0); \
         ANALYZE;",
    )
    .unwrap();

    let plan = v016_plan(
        &conn,
        "SELECT t.policy_id, SUM(CASE t.kind WHEN 'expense' THEN t.amount_native_cents \
         ELSE 0 END) FROM transactions t JOIN policies p ON p.id=t.policy_id \
         WHERE t.is_deleted=0 AND t.kind IN ('expense') AND p.is_deleted=0 \
         GROUP BY t.policy_id",
        [],
    );
    assert!(
        plan.contains("idx_transactions_policy"),
        "保单统计应由保单部分覆盖索引驱动: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE"),
        "GROUP BY 应由索引序满足，无临时 B-tree: {plan}"
    );
}

#[test]
fn v025_zero_consumer_indexes_dropped() {
    let conn = test_support::open();

    for name in [
        "idx_transactions_amount",
        "idx_transactions_refund",
        "idx_transactions_sync",
        "idx_transactions_account",
        "idx_transactions_category",
        "idx_transactions_deleted",
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                [name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "零消费者索引 {name} 应已随 V025 删除");
    }
}

fn v038_world() -> Connection {
    let conn = test_support::open();

    seed_account(&conn, "acc-01", "现金", "cash", "CNY", 0);
    seed_account(&conn, "acc-02", "储蓄卡", "bank", "CNY", 0);
    seed_account(&conn, "acc-inv", "证券户", "investment", "CNY", 0);
    conn.execute_batch(
        "WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM seq WHERE i<20000)\
         INSERT INTO transactions\
           (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
            funding_account_id,note,date,created_at,updated_at,version,device_id,is_deleted)\
         SELECT printf('tx-%05d', i),\
                CASE WHEN i%1000<830 THEN 'expense' WHEN i%1000<850 THEN 'income'\
                     WHEN i%1000<890 THEN 'transfer' WHEN i%1000<900 THEN 'refund'\
                     WHEN i%1000<955 THEN 'buy' WHEN i%1000<975 THEN 'sell'\
                     WHEN i%1000<990 THEN 'dividend' WHEN i%1000<997 THEN 'convert'\
                     ELSE 'split' END,\
                100, 'CNY', 100,\
                CASE WHEN i%3=0 THEN 'acc-inv' WHEN i%3=1 THEN 'acc-01' ELSE 'acc-02' END,\
                CASE WHEN i%10=7 THEN 'acc-01' ELSE NULL END,\
                NULL, NULL,\
                date('2026-03-01', '-' || (i%90) || ' days'),\
                '2026-03-01T08:00:00Z', '2026-03-01T08:00:00Z',\
                1, 'test', CASE WHEN i%100=0 THEN 1 ELSE 0 END \
         FROM seq;\
         INSERT INTO transaction_fundings (transaction_id, sort, account_id, amount_cents)\
         VALUES ('tx-00001', 0, 'acc-inv', 60), ('tx-00002', 0, 'acc-inv', 60);\
         ANALYZE;",
    )
    .unwrap();
    conn
}

#[test]
fn v038_hide_investment_count_uses_kind_count_covering_index() {
    let conn = v038_world();

    let plan = v016_plan(
        &conn,
        "SELECT COUNT(*) FROM transactions WHERE is_deleted=0 AND kind IN (?,?,?,?) \
         AND NOT EXISTS (SELECT 1 FROM accounts a WHERE a.type = 'investment' AND a.id = transactions.account_id) \
         AND NOT EXISTS (SELECT 1 FROM accounts a WHERE a.type = 'investment' AND a.id = transactions.to_account_id) \
         AND NOT EXISTS (SELECT 1 FROM accounts a WHERE a.type = 'investment' AND a.id = transactions.funding_account_id) \
         AND NOT EXISTS (SELECT 1 FROM transaction_fundings f \
         JOIN accounts a ON a.id = f.account_id AND a.type = 'investment' \
         WHERE f.transaction_id = transactions.id)",
        ["expense", "income", "transfer", "refund"],
    );
    assert!(
        plan.contains("idx_transactions_kind_count"),
        "hide_investment COUNT 应由 V038 覆盖索引驱动: {plan}"
    );
    assert!(
        !plan.contains("SCAN transactions"),
        "外层不应全表扫描肥表: {plan}"
    );
}

#[test]
fn perf_trace_sql_event_inherits_command_span() {
    let conn = test_support::open();

    let events = capture_events(|| {
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
