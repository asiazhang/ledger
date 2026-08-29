//! 迁移、种子与 schema 约束测试：迁移集合自校验、`init_db` 幂等与默认种子、
//! 表级唯一约束（exchange_rates 货币对唯一、V010 price/fx history 周采样唯一）
//! 与旧版本备份升级路径。

use rusqlite::params;

use crate::db::{init_db, migrations, open_in_memory};

use super::common::{insert_fx_rate_history, insert_instrument, insert_price_history};

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

// ---------------------------------------------------------------------------
// V010：价格历史化（issue #136 / ADR-0019）——price_history 与 fx_rate_history
// ---------------------------------------------------------------------------

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
