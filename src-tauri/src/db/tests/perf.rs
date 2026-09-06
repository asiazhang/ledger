//! 性能相关测试：`security_lots` 聚合覆盖索引的实际命中、耗时分级纯函数边界
//! 与 perf trace hook 接线（ADR-0009）。

use std::time::Duration;

use rusqlite::Connection;
use tracing::Level;

use crate::db::{init_db, open_in_memory, perf_trace};
use crate::test_utils::capture_events;

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

// ---------------------------------------------------------------------------
// V016 结构索引与统计（issue #490）——EXPLAIN 绑定测试
// ---------------------------------------------------------------------------

/// 捕获 EXPLAIN QUERY PLAN 的 detail 列（与上方先例同款列序）。
/// 占位符按调用方实参绑定（如时点持仓的 `?1`/`?2`），保证计划与真实调用一致。
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

/// V016 EXPLAIN 绑定测试世界：迁移后内存库 + 两账户 + 一分类 + 约 2000 笔交易
/// （日期跨 3 个月、账户/分类轮转、约 1% 软删，与真实画像同分布）+ 一笔买入
/// 证券交易；迁移尾部 ANALYZE 在空表运行，此处随数据重算——与存量用户升级、
/// 基准库生成后的统计形态一致，保证 planner 选择可代表真实库。
fn v016_world() -> Connection {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    conn.execute_batch(
        "INSERT INTO accounts (id,name,type,currency_code,created_at,updated_at,version,device_id) \
         VALUES ('acc-01','现金','cash','CNY','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test'),\
                 ('acc-02','储蓄卡','bank','CNY','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test');\
         INSERT INTO categories (id,name,kind,created_at,updated_at,version,device_id) \
         VALUES ('cat-01','餐饮','expense','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test');\
         INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES ('inst-01','600000.SH','stock','浦发银行','CNY','sh','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test');\
         WITH RECURSIVE seq(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM seq WHERE i<2000)\
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

/// V016 六条新索引存在且均为 partial（WHERE is_deleted=0）+ 全部覆盖列齐备。
#[test]
fn v016_structural_indexes_exist_with_covering_columns() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

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

/// 列表/深分页钉计划：列表序索引驱动 + 无 ORDER BY 临时 B-tree
/// （列表 SQL 与 ADR-0008 契约零改动，排序语义由索引反向扫描满足）。
#[test]
fn v016_list_pagination_uses_list_order_index_without_temp_btree() {
    let conn = v016_world();
    let sql = "SELECT id,kind,amount_cents,currency_code,amount_native_cents,account_id,\
               to_account_id,category_id,refund_of_transaction_id,note,date,created_at,\
               updated_at,version,device_id,is_deleted,merchant_id,policy_id \
               FROM transactions WHERE is_deleted=0 \
               ORDER BY date DESC, created_at DESC, id DESC";

    // 首页（LIMIT 早停）与深分页（大 OFFSET）同计划形状。
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

/// 账户 × 日期窗口筛选钉计划：账户筛选序索引覆盖定位与排序（无临时 B-tree）。
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

/// 账户现金流钉计划：转出侧（account_id）与转入侧（to_account_id）余额聚合
/// 各自命中对应覆盖索引（kind 与金额全在索引内，不回表）。
#[test]
fn v016_balance_flow_aggregates_use_cash_flow_indexes() {
    let conn = v016_world();
    // 转出侧：account_flow（Out）聚合形状与 accounts/balance.rs 同构。
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
    // 转入侧：account_flow（In）聚合形状同构，走 to_account_id 索引。
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

/// 月度汇总钉计划：INDEXED BY 钉定的表达式索引驱动分组（无 GROUP BY 临时 B-tree），
/// 期间与遗留年份两条口径同计划（钉定后 planner 不再随统计边际摇摆）。
#[test]
fn v016_monthly_summary_uses_pinned_expression_index() {
    let conn = v016_world();
    // 期间口径（from/to 均存在，基准口径）。
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

    // 遗留年份口径（substr(date,1,4)=?）。
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

/// 分类聚合钉计划：分类覆盖索引驱动 GROUP BY（分类/类型/日期/金额全在索引内）。
#[test]
fn v016_category_shares_use_category_covering_index() {
    let conn = v016_world();
    // 与 reports 域分类聚合同形状（含 ORDER BY net 的结果排序步骤）。
    let plan = v016_plan(
        &conn,
        "SELECT t.category_id, SUM(CASE WHEN t.kind='expense' THEN t.amount_native_cents \
         WHEN t.kind='refund' THEN -t.amount_native_cents ELSE 0 END) \
         FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind IN ('expense','refund') AND t.is_deleted=0 \
         GROUP BY t.category_id ORDER BY 2 DESC",
        [],
    );
    assert!(
        plan.contains("idx_transactions_category_covering"),
        "分类聚合应由分类覆盖索引驱动: {plan}"
    );
}

/// 日期探测钉计划：改写后的两个标量子查询各自经列表序索引端点定位
/// （单条 MIN+MAX 无法双向走索引，拆开后亚毫秒）。
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

/// 时点持仓钉计划：join 从 security_transactions 侧驱动（外层循环），
/// 依赖迁移尾部 ANALYZE 与导入后 PRAGMA optimize 维持的统计假设。
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

// ---------------------------------------------------------------------------
// V001 dedup_hash 兜底索引（issue #701 / #532 归因）——导入去重兜底查询的
// EXPLAIN 绑定测试（索引经 V001 就地修改引入，全新安装生效）
// ---------------------------------------------------------------------------

/// dedup_hash 部分索引存在：列序 (dedup_hash, created_at) + 双条件部分谓词
/// （软删行与 NULL 哈希不进索引，与兜底查询谓词精确匹配，对齐幂等键部分
/// 唯一索引形态）。
#[test]
fn v001_dedup_hash_partial_index_exists() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

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

/// 导入去重兜底查询（`batch.rs::dedup_identity` 无键路径原句）钉计划：
/// dedup_hash 部分索引驱动等值定位，ORDER BY created_at 由索引列序满足——
/// 无临时 B-tree（单列形态 EXPLAIN 实证会退化 USE TEMP B-TREE FOR ORDER BY，
/// created_at 并入索引列后消除，issue #701）。
#[test]
fn v001_dedup_fallback_query_uses_dedup_hash_index_without_temp_btree() {
    let conn = v016_world();
    // v016_world 的 dedup_hash 全 NULL（partial 索引之外）；补齐一部分活跃哈希
    // 并重算统计，保证 planner 选择可代表真实导入库（有键行分布）。
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
        plan.contains("idx_transactions_dedup_hash"),
        "兜底查询应命中 dedup_hash 部分索引: {plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE"),
        "ORDER BY created_at 应由索引列序满足，无临时 B-tree: {plan}"
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
