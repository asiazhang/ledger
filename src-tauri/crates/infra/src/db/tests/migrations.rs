use rusqlite::{Connection, params};

use crate::db::migrations;
use tauri_app_lib::test_support::{
    FIXED_NOW, seed_account, seed_exchange_rate, seed_fx_rate_history, seed_instrument,
    seed_price_history,
};

use super::common::{
    probe_exchange_rate, probe_fx_rate_history, probe_instrument_market, probe_instrument_source,
    probe_price_history,
};

#[test]
fn init_db_is_idempotent_and_seeds_defaults() {
    let mut conn = tauri_app_lib::test_support::open();
    migrations().to_latest(&mut conn).unwrap();
    migrations().to_latest(&mut conn).unwrap();

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

const LATEST_SCHEMA_VERSION: usize = 37;

#[test]
fn guard_schema_ahead_rejects_db_newer_than_chain() {
    let conn = tauri_app_lib::test_support::open();
    let ahead = (LATEST_SCHEMA_VERSION + 1) as i64;
    conn.pragma_update(None, "user_version", ahead).unwrap();
    let err = crate::db::migrate::guard_schema_ahead(&conn).unwrap_err();
    assert_eq!(err.code(), Some(crate::db::migrate::BOOT_SCHEMA_AHEAD));
    let kept: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, ahead, "user_version 应原样保留（不降级、不动库）");
    conn.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION as i64)
        .unwrap();
    crate::db::migrate::guard_schema_ahead(&conn).unwrap();
}

#[test]
fn migration_from_zero_reaches_latest_completely() {
    let conn = tauri_app_lib::test_support::open();

    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        user_version, LATEST_SCHEMA_VERSION as i64,
        "从零迁移应到达最新 schema 版本 {LATEST_SCHEMA_VERSION}"
    );

    crate::db::check_integrity(&conn)
        .unwrap_or_else(|e| panic!("从零迁移后的库应通过完整性检查: {e}"));

    for table in [
        "accounts",
        "categories",
        "currencies",
        "merchants",
        "transactions",
        "exchange_rates",
        "budgets",
        "app_settings",
        "items",
        "instruments",
        "price_history",
        "fx_rate_history",
        "policies",
        "scheduled_transactions",
        "physical_assets",
        "insurers",
        "security_lot_conversions",
        "security_lot_adjustments",
        "account_balance_cache",
        "net_worth_cache",
        "goals",
        "transaction_fundings",
        "transaction_purchases",
    ] {
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1, "签名表 {table} 应存在");
    }

    for table in [
        "sync_device",
        "sync_ops",
        "sync_parked_ops",
        "sync_stream_positions",
    ] {
        let attached: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync.sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(attached, 1, "签名表 {table} 应迁入 attached 侧");
        let main_hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM main.sqlite_master WHERE type='table' AND name=?1",
                params![table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(main_hit, 0, "签名表 {table} 不应留在主库");
    }

    for (table, column) in [
        ("transactions", "merchant_id"),
        ("transactions", "idempotency_key"),
        ("transactions", "policy_id"),
        ("transactions", "funding_account_id"),
        ("instruments", "source"),
        ("subscription_plans", "policy_id"),
        ("sync.sync_parked_ops", "park_params"),
        ("security_transactions", "to_instrument_id"),
        ("accounts", "is_archived"),
        ("accounts", "credit_limit_cents"),
        ("accounts", "statement_day"),
        ("accounts", "due_day"),
        ("security_transactions", "origin"),
        ("transactions", "fx_rate_used"),
        ("transactions", "fx_rate_source"),
        ("transactions", "source_order_no"),
        ("transaction_purchases", "name"),
        ("transaction_purchases", "quantity"),
        ("transaction_purchases", "category_id"),
        ("transaction_purchases", "unit_price_cents"),
    ] {
        let (schema, table_name) = match table.split_once('.') {
            Some((s, t)) => (s, t),
            None => ("main", table),
        };
        let hit: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {schema}.pragma_table_info(?1) WHERE name=?2"),
                params![table_name, column],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1, "签名列 {table}.{column} 应存在");
    }

    let index_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master \
             WHERE type='index' AND name='idx_transactions_note_search'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("索引 idx_transactions_note_search 应存在"));
    let index_ddl: String = index_sql.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        index_ddl.contains(
            "ONtransactions(date,created_at,id,note,account_id,merchant_id,category_id)",
        ),
        "idx_transactions_note_search 列集应为列表序键 + id/note/三引用列: {index_sql}"
    );
    assert!(
        index_ddl.contains("WHEREis_deleted=0"),
        "idx_transactions_note_search 应保持 partial 谓词: {index_sql}"
    );
    assert!(
        !index_ddl.contains("note_pinyin"),
        "idx_transactions_note_search 不得再引用已退役的 note_pinyin（#1728）: {index_sql}"
    );

    let backlog_index: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master \
             WHERE type='index' AND name='idx_transactions_note_pinyin_backlog'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        backlog_index, 0,
        "回填探针索引 idx_transactions_note_pinyin_backlog 应已随 note_pinyin 退役"
    );

    let note_pinyin_column: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('transactions') WHERE name='note_pinyin'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        note_pinyin_column, 0,
        "派生列 transactions.note_pinyin 应已随 V031 移除"
    );

    let merchant_covering_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master \
             WHERE type='index' AND name='idx_transactions_merchant_covering'",
            [],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| panic!("索引 idx_transactions_merchant_covering 应存在"));
    for col in ["merchant_id", "kind", "date", "amount_native_cents"] {
        assert!(
            merchant_covering_sql.contains(col),
            "idx_transactions_merchant_covering 应包含 {col}: {merchant_covering_sql}"
        );
    }
    assert!(
        merchant_covering_sql.contains("is_deleted = 0"),
        "idx_transactions_merchant_covering 应为 partial index: {merchant_covering_sql}"
    );
}

fn insert_raw_transaction(
    conn: &Connection,
    id: &str,
    kind: &str,
    account_id: &str,
) -> rusqlite::Result<usize> {
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,\
          created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,0,'CNY',0,?3,'2026-05-11',?4,?4,1,'test',0)",
        params![id, kind, account_id, FIXED_NOW],
    )
}

#[test]
fn convert_schema_shape_is_complete_from_zero() {
    let conn = tauri_app_lib::test_support::open();
    seed_account(&conn, "acc-convert", "投资账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-out", "000001.OF", "基金A", "CNY", "unknown");
    seed_instrument(&conn, "inst-in", "000002.OF", "基金B", "CNY", "unknown");

    insert_raw_transaction(&conn, "tx-convert", "convert", "acc-convert")
        .unwrap_or_else(|e| panic!("kind='convert' 应可落库: {e}"));
    let bogus_kind = insert_raw_transaction(&conn, "tx-bogus", "convert_typo", "acc-convert");
    assert!(bogus_kind.is_err(), "闭集外 kind 应被 CHECK 拒绝");

    conn.execute(
        "INSERT INTO security_transactions \
         (transaction_id,instrument_id,action,quantity,price_cents,fee_cents,\
          to_instrument_id,to_quantity,out_amount_cents,in_amount_cents) \
         VALUES ('tx-convert','inst-out','convert',100.0,10000,0,\
                 'inst-in',50.0,10000,10000)",
        [],
    )
    .unwrap_or_else(|e| panic!("action='convert' 与转换扩展列应可落库: {e}"));

    insert_raw_transaction(&conn, "tx-convert-bad", "convert", "acc-convert").unwrap();
    let bogus_action = conn.execute(
        "INSERT INTO security_transactions \
         (transaction_id,instrument_id,action,quantity,fee_cents) \
         VALUES ('tx-convert-bad','inst-out','convert_typo',1.0,0)",
        [],
    );
    assert!(bogus_action.is_err(), "闭集外 action 应被 CHECK 拒绝");

    let hard_delete = conn.execute("DELETE FROM instruments WHERE id='inst-in'", []);
    assert!(
        hard_delete.is_err(),
        "被转换行引用的转入标的硬删应被 RESTRICT 拒绝"
    );

    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('security_lot_conversions') ORDER BY cid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        vec![
            "id",
            "transaction_id",
            "lot_id",
            "quantity",
            "cost_per_unit_cents",
            "cost_cents",
            "created_at"
        ],
        "security_lot_conversions 列集应与转换消耗表契约一致"
    );
}

#[test]
fn split_lot_adjustments_schema_shape_is_complete_from_zero() {
    let conn = tauri_app_lib::test_support::open();
    seed_account(&conn, "acc-split", "投资账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-split", "502010", "基金S", "CNY", "unknown");

    insert_raw_transaction(&conn, "tx-split", "split", "acc-split")
        .unwrap_or_else(|e| panic!("kind='split' 应可落库（闭集早已声明）: {e}"));
    conn.execute(
        "INSERT INTO security_transactions \
         (transaction_id,instrument_id,action,quantity,fee_cents) \
         VALUES ('tx-split','inst-split','split',339.76,0)",
        [],
    )
    .unwrap_or_else(|e| panic!("action='split' 行应可落库（price_cents 留 NULL）: {e}"));
    conn.execute(
        "INSERT INTO security_lots \
         (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES ('lot-split','acc-split','inst-split','tx-split',100.0,100.0,10000,'CNY',?1,?1,1,'test')",
        params![FIXED_NOW],
    )
    .unwrap();

    let columns: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('security_lot_adjustments') ORDER BY cid")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        columns,
        vec![
            "id",
            "transaction_id",
            "lot_id",
            "initial_quantity_before",
            "initial_quantity_after",
            "remaining_quantity_before",
            "remaining_quantity_after",
            "cost_per_unit_cents_before",
            "cost_per_unit_cents_after",
            "created_at"
        ],
        "security_lot_adjustments 列集应与重述审计表契约一致"
    );

    conn.execute(
        "INSERT INTO security_lot_adjustments \
         (id,transaction_id,lot_id,initial_quantity_before,initial_quantity_after,\
          remaining_quantity_before,remaining_quantity_after,cost_per_unit_cents_before,\
          cost_per_unit_cents_after,created_at) \
         VALUES ('adj-01','tx-split','lot-split',100.0,106.2,100.0,106.2,10000,9416,?1)",
        params![FIXED_NOW],
    )
    .unwrap_or_else(|e| panic!("重述审计行应可落库: {e}"));

    conn.execute(
        "DELETE FROM security_transactions WHERE transaction_id='tx-split'",
        [],
    )
    .unwrap();
    let left: i64 = conn
        .query_row("SELECT COUNT(*) FROM security_lot_adjustments", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(left, 0, "split 扩展行删除后重述审计行应级联消失");
}

#[test]
fn policies_table_references_insurers() {
    let conn = tauri_app_lib::test_support::open();

    let has_insurer_id: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('policies') WHERE name='insurer_id')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let has_merchant_id: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('policies') WHERE name='merchant_id')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(has_insurer_id, "policies 表应有 insurer_id 列（保司引用）");
    assert!(
        !has_merchant_id,
        "policies 表不应再有 merchant_id 列（商户引用已换轨）"
    );

    conn.execute(
        "INSERT INTO policies (id,insurer_id,policy_number,product_name,start_date,\
         created_at,updated_at,version,device_id,is_deleted) \
         SELECT 'pol-01', id, 'P-1', '重疾险', '2026-01-01', \
         ?1,?2,1,'test',0 \
         FROM insurers WHERE name='平安人寿' AND is_deleted=0",
        params![FIXED_NOW, FIXED_NOW],
    )
    .unwrap_or_else(|e| panic!("引用种子保司应可落库: {e}"));
    let dangling = conn.execute(
        "INSERT INTO policies (id,insurer_id,policy_number,product_name,start_date,\
         created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('pol-02','ins-nothing','P-2','医疗险','2026-01-01',\
         ?1,?2,1,'test',0)",
        params![FIXED_NOW, FIXED_NOW],
    );
    assert!(dangling.is_err(), "引用不存在保司应被外键拒绝");

    let hard_delete = conn.execute("DELETE FROM insurers WHERE name='平安人寿'", []);
    assert!(
        hard_delete.is_err(),
        "被保单引用的保司硬删应被 RESTRICT 拒绝"
    );
}

#[test]
fn insurer_seed_is_present_and_idempotent() {
    let mut conn = tauri_app_lib::test_support::open();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM insurers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 30, "迁移后应预置 30 家常用保司");

    for name in [
        "中国人寿",
        "平安人寿",
        "中汇人寿",
        "人保财险",
        "平安财险",
        "国寿财险",
        "众安保险",
    ] {
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM insurers WHERE name=?1 AND is_deleted=0",
                params![name],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1, "预置保司应包含 '{name}'");
    }

    let id: String = conn
        .query_row(
            "SELECT id FROM insurers WHERE name='中国人寿'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(id, "dc5b304a-d85d-5d5f-a749-c92894e92a41");

    let (version, device_id, is_deleted): (i64, String, i64) = conn
        .query_row(
            "SELECT version, device_id, is_deleted FROM insurers WHERE name='中国人寿'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(device_id, "seed");
    assert_eq!(is_deleted, 0);

    migrations().to_latest(&mut conn).unwrap();
    conn.execute_batch(include_str!(
        "../../../../../migrations/V019__insurer_dictionary.sql"
    ))
    .unwrap();
    conn.execute_batch(include_str!(
        "../../../../../migrations/V019__insurer_dictionary.sql"
    ))
    .unwrap();
    let count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM insurers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_after, 30, "种子重复执行应幂等：同名不重复建");

    let dup = conn.execute(
        "INSERT INTO insurers (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('ins-dup','中国人寿','2026-09-07T00:00:00Z','2026-09-07T00:00:00Z',1,'test',0)",
        [],
    );
    assert!(dup.is_err(), "在用行同名应违反唯一约束");
    conn.execute("UPDATE insurers SET is_deleted=1 WHERE name='中国人寿'", [])
        .unwrap();
    conn.execute(
        "INSERT INTO insurers (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('ins-new','中国人寿','2026-09-07T00:00:00Z','2026-09-07T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap_or_else(|e| panic!("软删后同名可再建: {e}"));
}

#[test]
fn exchange_rate_single_row_per_pair() {
    let conn = tauri_app_lib::test_support::open();

    seed_exchange_rate(&conn, "USD", "CNY", 7.2);

    let dup = probe_exchange_rate(&conn, "er-02", "USD", "CNY", 7.0);
    assert!(dup.is_err(), "同货币对第二行应违反唯一约束");
}

#[test]
fn price_history_weekly_unique_and_cascade() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument(&conn, "inst-01", "600519.SH", "贵州茅台", "CNY", "sh");

    seed_price_history(&conn, "ph-01", "inst-01", "2026-05-27", 170000, "CNY");
    let dup = probe_price_history(&conn, "ph-02", "inst-01", "2026-05-27", 170000);
    assert!(dup.is_err(), "同标的同采样日第二行应违反周唯一约束");
    let dup_same_week = probe_price_history(&conn, "ph-02b", "inst-01", "2026-05-29", 171000);
    assert!(
        dup_same_week.is_err(),
        "同周不同采样日第二行应违反周唯一约束"
    );
    seed_price_history(&conn, "ph-03", "inst-01", "2026-06-03", 170000, "CNY");

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);

    conn.execute("DELETE FROM instruments WHERE id='inst-01'", [])
        .unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "删除标的后价格历史应级联删除");
}

#[test]
fn instruments_source_defaults_eastmoney_and_rejects_null() {
    let conn = tauri_app_lib::test_support::open();

    probe_instrument_market(&conn, "inst-d", "600000", "sh")
        .unwrap_or_else(|e| panic!("省略 source 的标的应可落库: {e}"));
    let source: String = conn
        .query_row(
            "SELECT source FROM instruments WHERE id='inst-d'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, "eastmoney", "省略 source 的新写入应落列默认值");

    let null_rejected = probe_instrument_source(&conn, "inst-null", "000002", "sz", None);
    assert!(null_rejected.is_err(), "source 列 NOT NULL 应拒绝显式 NULL");
}

#[test]
fn instruments_market_check_accepts_us_markets() {
    let conn = tauri_app_lib::test_support::open();

    for (i, market) in ["sh", "sz", "hk", "nasdaq", "nyse", "amex", "unknown"]
        .into_iter()
        .enumerate()
    {
        probe_instrument_market(&conn, &format!("inst-{i}"), &format!("SYM{i}"), market)
            .unwrap_or_else(|e| panic!("market {market} 应可落库: {e}"));
    }

    let rejected = probe_instrument_market(&conn, "inst-x", "LSE", "lse");
    assert!(rejected.is_err(), "闭集外 market 应被 CHECK 拒绝");
}

#[test]
fn fx_rate_history_weekly_unique_per_pair() {
    let conn = tauri_app_lib::test_support::open();

    seed_fx_rate_history(&conn, "fx-01", "HKD", "CNY", "2026-05-27", 0.92);
    let dup = probe_fx_rate_history(&conn, "fx-02", "HKD", "CNY", "2026-05-27", 0.92);
    assert!(dup.is_err(), "同币种对同采样日第二行应违反周唯一约束");
    let dup_same_week = probe_fx_rate_history(&conn, "fx-02b", "HKD", "CNY", "2026-05-29", 0.93);
    assert!(dup_same_week.is_err(), "同币种对同周第二行应违反周唯一约束");
    seed_fx_rate_history(&conn, "fx-03", "HKD", "CNY", "2026-06-03", 0.92);
    seed_fx_rate_history(&conn, "fx-04", "CNY", "HKD", "2026-05-27", 1.087);
}

fn insert_scheduled_plan(conn: &Connection, id: &str, kind: &str, category_id: Option<&str>) {
    conn.execute(
        "INSERT INTO scheduled_transactions \
         (id, kind, status, account_id, category_id, amount_cents, currency_code, \
          recurrence_type, recurrence_interval, recurrence_day, start_date, note, \
          created_at, updated_at, version, device_id, is_deleted) \
         VALUES (?1, ?2, 'active', 'acc-01', ?3, 1500, 'CNY', \
                 'monthly', 1, 1, '2026-06-01', NULL, \
                 ?4, ?4, 1, 'test', 0)",
        rusqlite::params![id, kind, category_id, FIXED_NOW],
    )
    .unwrap();
}

fn insert_occurrence(conn: &Connection, id: &str, plan_id: &str) {
    conn.execute(
        "INSERT INTO scheduled_transaction_occurrences \
         (id, scheduled_transaction_id, scheduled_date, status, transaction_id, \
          amount_cents, created_at, updated_at, version, device_id, is_deleted) \
         VALUES (?1, ?2, '2026-06-01', 'pending', NULL, \
                 1500, ?3, ?3, 1, 'test', 0)",
        rusqlite::params![id, plan_id, FIXED_NOW],
    )
    .unwrap();
}

fn world_with_account() -> Connection {
    let conn = tauri_app_lib::test_support::open();
    seed_account(&conn, "acc-01", "现金", "cash", "CNY", 0);
    conn
}

fn world_with_three_plans() -> Connection {
    let conn = world_with_account();
    insert_scheduled_plan(&conn, "st-01", "installment", None);
    insert_scheduled_plan(&conn, "st-02", "subscription", None);
    insert_scheduled_plan(&conn, "st-03", "scheduled_transfer", None);
    for id in ["st-01", "st-02", "st-03"] {
        insert_occurrence(&conn, &format!("occ-{id}"), id);
    }
    conn.execute(
        "INSERT INTO installment_plans (scheduled_transaction_id, total_amount_cents, total_occurrences) \
         VALUES ('st-01', 3000, 2)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO subscription_plans (scheduled_transaction_id) VALUES ('st-02')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO scheduled_transfer_plans (scheduled_transaction_id, to_account_id, total_occurrences) \
         VALUES ('st-03', 'acc-01', NULL)",
        [],
    )
    .unwrap();
    conn
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn migration_audit_every_foreign_key_has_explicit_on_delete() {
    let conn = tauri_app_lib::test_support::open();

    let tables: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    const ON_DELETE: usize = 6;
    const FROM_COLUMN: usize = 3;
    const ALLOWED: [&str; 3] = ["RESTRICT", "CASCADE", "SET NULL"];

    let mut checked = 0usize;
    for table in &tables {
        let fk_sql = format!(r#"PRAGMA foreign_key_list("{table}")"#);
        let fks: Vec<(String, String)> = conn
            .prepare(&fk_sql)
            .unwrap()
            .query_map([], |r| Ok((r.get(FROM_COLUMN)?, r.get(ON_DELETE)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for (from_column, on_delete) in fks {
            checked += 1;
            assert!(
                ALLOWED.contains(&on_delete.as_str()),
                "表 {table} 列 {from_column} 的外键未显式声明 ON DELETE（落到 SQLite 默认 NO ACTION），实际动作: {on_delete}"
            );
        }
    }
    assert!(
        checked >= 30,
        "审计应覆盖全库全部外键，实际仅反射到 {checked} 条"
    );
}

#[test]
fn hard_delete_category_nulls_scheduled_plan_category() {
    let conn = world_with_account();
    conn.execute(
        "INSERT INTO categories (id, name, kind, created_at, updated_at, version, device_id) \
         VALUES ('cat-01', '餐饮', 'expense', ?1, ?1, 1, 'test')",
        params![FIXED_NOW],
    )
    .unwrap();
    insert_scheduled_plan(&conn, "st-01", "subscription", Some("cat-01"));

    conn.execute("DELETE FROM categories WHERE id = 'cat-01'", [])
        .unwrap();

    let (category_id, n): (Option<String>, i64) = conn
        .query_row(
            "SELECT category_id, COUNT(*) FROM scheduled_transactions WHERE id = 'st-01'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(n, 1, "硬删分类不应连带删除定时交易");
    assert_eq!(category_id, None, "定时交易的分类引用应被置空");
}

#[test]
fn hard_delete_scheduled_plan_cascades_occurrences_and_extensions() {
    let conn = world_with_three_plans();
    assert_eq!(count(&conn, "scheduled_transaction_occurrences"), 3);

    conn.execute("DELETE FROM scheduled_transactions", [])
        .unwrap();

    assert_eq!(
        count(&conn, "scheduled_transaction_occurrences"),
        0,
        "期次应随计划级联删除"
    );
    assert_eq!(count(&conn, "installment_plans"), 0, "分期扩展行应级联删除");
    assert_eq!(
        count(&conn, "subscription_plans"),
        0,
        "订阅扩展行应级联删除"
    );
    assert_eq!(
        count(&conn, "scheduled_transfer_plans"),
        0,
        "定时转账扩展行应级联删除"
    );
}

#[test]
fn hard_delete_account_with_scheduled_plan_is_restricted() {
    let conn = world_with_three_plans();

    let result = conn.execute("DELETE FROM accounts WHERE id = 'acc-01'", []);
    assert!(
        result.is_err(),
        "被定时交易强引用的账户硬删应被 RESTRICT 拒绝"
    );
    assert_eq!(count(&conn, "scheduled_transactions"), 3, "计划应全部保留");
}
