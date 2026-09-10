//! 迁移、种子与 schema 约束测试。策略（2026-09-08 删库重启迁移失败调查后收口）：
//! 全部以「内存库从零迁移到最新」为唯一形态——user_version 基线、完整性检查、
//! 签名表/列/索引、`init_db` 幂等与默认种子、表级唯一约束（exchange_rates 货币对
//! 唯一、V010 price/fx history 周采样唯一）、全库外键显式 ON DELETE 审计与定时
//! 交易系删除行为抽查（issue #273 / spec #271）。
//!
//! 历史库升级路径不测：V001 等已发布迁移被就地修改后，真实历史 schema（如
//! user_version=9 缺 merchant_id 的残留库）无法由现有迁移文件复现，旧式升级
//! 测试只能覆盖虚构 schema（假信心，本次事故即漏网）；该输入由启动失败恢复
//! 屏接管（重置/恢复通道）。迁移文本里的存量数据回填表达式（如 V017 缓存回填）
//! 在从零形态无存量行可回填，不再单独锁定——未来新增带存量回填语义的迁移
//! 时再议。

use rusqlite::{Connection, params};

use crate::db::migrations;
use crate::test_support::{
    FIXED_NOW, seed_account, seed_exchange_rate, seed_fx_rate_history, seed_instrument,
    seed_price_history,
};

use super::common::{
    probe_exchange_rate, probe_fx_rate_history, probe_instrument_market, probe_instrument_source,
    probe_price_history,
};

/// init_db 应幂等：连续跑两次不报错，且默认币种 11 条、分类 92 条已写入
/// （18 顶级 + 74 二级）。
/// 建库经统一测试工厂（= open_in_memory + init_db 两行序）；幂等重入走产品
/// 迁移缝 `to_latest`（init_db 的迁移核心），spec #728 / issue #754 / ADR-0084 决策 7。
#[test]
fn init_db_is_idempotent_and_seeds_defaults() {
    let mut conn = crate::test_support::open();
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

/// 当前迁移序列长度（V001–V023，V005 移除不回填，共 22 条）；新增迁移时随
/// `migrations()` 同步更新。钉住「从零迁移到最新」的完整性基线。
const LATEST_SCHEMA_VERSION: usize = 22;

/// 从零迁移完整性（内存库从零 → 最新）：user_version 停在最新、全库完整性
/// 检查通过、每条迁移的签名表/列在场。漏跑或中途失败的迁移批次会停在半途
/// （迁移批次单事务回滚），本测试确定性失败。
#[test]
fn migration_from_zero_reaches_latest_completely() {
    let conn = crate::test_support::open();

    // 完整性：user_version 到达最新（迁移批次单事务，缺迁移即版本落后）。
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        user_version, LATEST_SCHEMA_VERSION as i64,
        "从零迁移应到达最新 schema 版本 {LATEST_SCHEMA_VERSION}"
    );

    // 正确性：SQLite 官方完整性判据（PRAGMA integrity_check 应为 ok）。
    crate::db::check_integrity(&conn)
        .unwrap_or_else(|e| panic!("从零迁移后的库应通过完整性检查: {e}"));

    // 签名表：每条建表迁移至少一个代表对象（漏建在此确定性失败）。
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
        "account_balance_cache",
        "net_worth_cache",
        "sync_device",
        "sync_ops",
        "sync_parked_ops",
        "sync_stream_positions",
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

    // 签名列：每条加列迁移的代表列。merchant_id 即 2026-09-08 事故列（就地
    // 修改 V001 引入、无前向迁移补列，历史残留 schema 迁移到 V018 必炸，
    // 由启动失败恢复屏接管）——从零路径在此钉住。
    for (table, column) in [
        ("transactions", "merchant_id"),
        ("transactions", "idempotency_key"),
        ("transactions", "policy_id"),
        ("transactions", "note_pinyin"),
        ("transactions", "funding_account_id"),
        ("instruments", "source"),
        ("subscription_plans", "policy_id"),
        // V021 就地修改（issue #957）：挂起原因的插值参数列。
        ("sync_parked_ops", "park_params"),
        // V002 就地修改（issue #977）：基金转换的转入腿列（同批另增 to_quantity /
        // out_amount_cents / in_amount_cents 与转出消耗表，形状锁见 convert 专测）。
        ("security_transactions", "to_instrument_id"),
    ] {
        let hit: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name=?2",
                params![table, column],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1, "签名列 {table}.{column} 应存在");
    }

    // 事故索引：V018 搜索覆盖索引在场，且定义文本引用 merchant_id
    // （索引列集漂移在此确定性失败）。
    let index_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master \
             WHERE type='index' AND name='idx_transactions_note_search'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        index_sql.contains("merchant_id"),
        "idx_transactions_note_search 定义应引用 merchant_id，实际: {index_sql}"
    );
}

// ---------------------------------------------------------------------------
// V001/V002 就地修改：基金转换（convert）schema（issue #977 / 父 spec #973 / ADR-0099）
// ---------------------------------------------------------------------------

/// 插入一条最小合法交易行（只带必填列，金额占位 0 与 convert/split 同臂；簿记戳
/// 引用工厂固定时刻常量，ADR-0084 决策 5）。为写入形态探针，返回 Result 由调用方判。
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

/// 基金转换 schema 形态（V001/V002 就地修改，issue #977）：全新安装即带第 9 种 kind
/// ——transactions.kind 与 security_transactions.action 闭集含 'convert'、闭集外取值
/// 仍被 CHECK 拒绝，转换扩展 4 列可写入，转出消耗表 security_lot_conversions 在场
/// 且列集与契约一致。**存量库不在兼容范围**：V001/V002 已发布，就地修改只影响全新
/// 安装，存量库须重建库（scripts/db-reset.sh）——裁定见两个迁移文件头部就地修改
/// 注记与 CHANGELOG「Unreleased」BREAKING 条目；历史库升级路径不测（见模块注释）。
#[test]
fn convert_schema_shape_is_complete_from_zero() {
    let conn = crate::test_support::open();
    seed_account(&conn, "acc-convert", "投资账户", "investment", "CNY", 0);
    seed_instrument(&conn, "inst-out", "000001.OF", "基金A", "CNY", "unknown");
    seed_instrument(&conn, "inst-in", "000002.OF", "基金B", "CNY", "unknown");

    // transactions.kind 闭集：'convert' 可落库，闭集外取值仍被拒。
    insert_raw_transaction(&conn, "tx-convert", "convert", "acc-convert")
        .unwrap_or_else(|e| panic!("kind='convert' 应可落库: {e}"));
    let bogus_kind = insert_raw_transaction(&conn, "tx-bogus", "convert_typo", "acc-convert");
    assert!(bogus_kind.is_err(), "闭集外 kind 应被 CHECK 拒绝");

    // security_transactions：action 含 'convert'，4 个转换列可写入（一行同时表达两腿：
    // instrument_id/quantity = 转出腿，to_instrument_id/to_quantity = 转入腿）。
    conn.execute(
        "INSERT INTO security_transactions \
         (transaction_id,instrument_id,action,quantity,price_cents,fee_cents,\
          to_instrument_id,to_quantity,out_amount_cents,in_amount_cents) \
         VALUES ('tx-convert','inst-out','convert',100.0,10000,0,\
                 'inst-in',50.0,10000,10000)",
        [],
    )
    .unwrap_or_else(|e| panic!("action='convert' 与转换扩展列应可落库: {e}"));

    // action 闭集外取值仍被拒（第二条交易行避开主键冲突）。
    insert_raw_transaction(&conn, "tx-convert-bad", "convert", "acc-convert").unwrap();
    let bogus_action = conn.execute(
        "INSERT INTO security_transactions \
         (transaction_id,instrument_id,action,quantity,fee_cents) \
         VALUES ('tx-convert-bad','inst-out','convert_typo',1.0,0)",
        [],
    );
    assert!(bogus_action.is_err(), "闭集外 action 应被 CHECK 拒绝");

    // 转入标的同款强依赖（RESTRICT）：被转换行引用的标的硬删被拒，不静默悬空。
    let hard_delete = conn.execute("DELETE FROM instruments WHERE id='inst-in'", []);
    assert!(
        hard_delete.is_err(),
        "被转换行引用的转入标的硬删应被 RESTRICT 拒绝"
    );

    // 转出消耗表在场且列集与契约一致（承载逐批次消耗与结转成本的审计载体）。
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

// ---------------------------------------------------------------------------
// V012 就地修改：保单表保司换轨（issue #713 / ADR-0082 决策 1/5）
// ---------------------------------------------------------------------------

/// 保单表保司字段换轨（V012 就地修改，issue #713）：全新安装的 policies 表以
/// `insurer_id` 引用保司字典（insurers，V019 建——DDL 允许前向引用，V012 与 V019
/// 之间无本表 DML），不再引用商户字典；外键动作保持 RESTRICT（档案存续依赖，
/// ADR-0051 决策 5 同款）。软删保司不可再建新档案引用（在用校验在行为层，
/// 库层只验存在性）。
#[test]
fn policies_table_references_insurers() {
    let conn = crate::test_support::open();

    // 新形状：insurer_id 在场、merchant_id 不在场（就地修改替换，非并存）。
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

    // 外键指向 insurers：引用在用保司可落库；引用不存在的保司被拒。
    // 簿记戳引用工厂固定时刻常量（ADR-0084 决策 5）。
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

    // 外键动作保持 RESTRICT：被保单引用的保司行硬删被拒（档案存续依赖）。
    let hard_delete = conn.execute("DELETE FROM insurers WHERE name='平安人寿'", []);
    assert!(
        hard_delete.is_err(),
        "被保单引用的保司硬删应被 RESTRICT 拒绝"
    );
}

// ---------------------------------------------------------------------------
// V019：保司字典（issue #712 / ADR-0082 决策 4）——insurers 表 + 常用保司种子
// ---------------------------------------------------------------------------

/// 保司字典种子：迁移后预置 30 家常用国内保司（人身险头部 + 财产险头部，覆盖
/// 车险场景）；种子行为普通字典行（可软删、无特殊标记）；init_db 重复执行幂等——
/// 同名不重复建（按名 INSERT OR IGNORE），行数与身份（确定性 UUID）稳定。
#[test]
fn insurer_seed_is_present_and_idempotent() {
    let mut conn = crate::test_support::open();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM insurers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 30, "迁移后应预置 30 家常用保司");

    // 人身险与财产险头部都在场（财产险覆盖车险场景是 ADR-0082 决策 4 的硬要求）。
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

    // 种子为确定性 UUID v5（所有设备一致，同步合并不产生重复字典行）：
    // 抽查「中国人寿」的 id 与迁移文件内登记值一致。
    let id: String = conn
        .query_row(
            "SELECT id FROM insurers WHERE name='中国人寿'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(id, "dc5b304a-d85d-5d5f-a749-c92894e92a41");

    // 种子行是普通字典行：version=1、device_id='seed'、在用（is_deleted=0）。
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

    // 幂等重跑：再次重入迁移链（init_db 因 user_version 已至最新不会重放 V019，
    // 重放 SQL 本体才能真验种子语句集的幂等性：IF NOT EXISTS + OR IGNORE）。
    migrations().to_latest(&mut conn).unwrap();
    conn.execute_batch(include_str!(
        "../../../migrations/V019__insurer_dictionary.sql"
    ))
    .unwrap();
    conn.execute_batch(include_str!(
        "../../../migrations/V019__insurer_dictionary.sql"
    ))
    .unwrap();
    let count_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM insurers", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_after, 30, "种子重复执行应幂等：同名不重复建");

    // 在用行全库唯一（partial unique index）：同名第二行被数据库拒绝。
    let dup = conn.execute(
        "INSERT INTO insurers (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('ins-dup','中国人寿','2026-09-07T00:00:00Z','2026-09-07T00:00:00Z',1,'test',0)",
        [],
    );
    assert!(dup.is_err(), "在用行同名应违反唯一约束");
    // 软删行不占名字：软删后同名可再建（字典语义照抄商户先例）。
    conn.execute("UPDATE insurers SET is_deleted=1 WHERE name='中国人寿'", [])
        .unwrap();
    conn.execute(
        "INSERT INTO insurers (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('ins-new','中国人寿','2026-09-07T00:00:00Z','2026-09-07T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap_or_else(|e| panic!("软删后同名可再建: {e}"));
}

/// 汇率表每货币对仅保留一行最新（UNIQUE(base_code, quote_code) 约束）。
/// 正反向查表与折算语义已收口到 Amount 接缝（`transaction::amount::convert_to_native`，
/// 见 transaction/tests.rs），此处不再重复。
#[test]
fn exchange_rate_single_row_per_pair() {
    let conn = crate::test_support::open();

    // 当前行：工厂汇率种子（行 id 由货币对派生，spec #728 / ADR-0084 决策 4）。
    seed_exchange_rate(&conn, "USD", "CNY", 7.2);

    // 同货币对第二行应被 UNIQUE(base_code, quote_code) 拒绝（探针直写：自定义
    // id，排除主键冲突干扰；探针见 db 测试域薄皮 common.rs）。
    let dup = probe_exchange_rate(&conn, "er-02", "USD", "CNY", 7.0);
    assert!(dup.is_err(), "同货币对第二行应违反唯一约束");
}

// ---------------------------------------------------------------------------
// V010：价格历史化（issue #136 / ADR-0019）——price_history 与 fx_rate_history
// ---------------------------------------------------------------------------

/// price_history：周采样唯一约束（每标的每周至多一条，同周不同采样日也拒绝）
/// + 标的级联删除跟随。
#[test]
fn price_history_weekly_unique_and_cascade() {
    let conn = crate::test_support::open();
    seed_instrument(&conn, "inst-01", "600519.SH", "贵州茅台", "CNY", "sh");

    seed_price_history(&conn, "ph-01", "inst-01", "2026-05-27", 170000, "CNY");
    // 同标的同采样日第二行应被周唯一约束拒绝（整周覆盖走 upsert，不产生重复；
    // 探针直写，自定义 id）。
    let dup = probe_price_history(&conn, "ph-02", "inst-01", "2026-05-27", 170000);
    assert!(dup.is_err(), "同标的同采样日第二行应违反周唯一约束");
    // 同周不同采样日（周三 vs 周五）同样应被拒绝——「每周至多一条」由库层强制。
    let dup_same_week = probe_price_history(&conn, "ph-02b", "inst-01", "2026-05-29", 171000);
    assert!(
        dup_same_week.is_err(),
        "同周不同采样日第二行应违反周唯一约束"
    );
    // 不同周（另一采样周）可正常写入。
    seed_price_history(&conn, "ph-03", "inst-01", "2026-06-03", 170000, "CNY");

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

// ---------------------------------------------------------------------------
// V011：标的字典来源列（issue #293 / ADR-0036 决策 2、ADR-0037 决策 5）
// ---------------------------------------------------------------------------

/// instruments.source 从零语义：列 NOT NULL + 默认 'eastmoney'——省略列的插入
/// 落默认值（UI 从无创建入口，字典均出自同步）；显式 NULL 被 NOT NULL 拒绝。
/// 词表 'eastmoney' | 'manual' 的闭集由写入通道收口，不在库层设 CHECK
/// （与价格侧 source 列同款）。历史库的升级回填语义不再单独测（见模块注释）。
#[test]
fn instruments_source_defaults_eastmoney_and_rejects_null() {
    let conn = crate::test_support::open();

    // 省略 source 列的插入落列默认值（market 探针不带 source 列，顺带覆盖 name 可空）。
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

    // 显式 NULL 被 NOT NULL 拒绝（探针直写，source 列显式入列）。
    let null_rejected = probe_instrument_source(&conn, "inst-null", "000002", "sz", None);
    assert!(null_rejected.is_err(), "source 列 NOT NULL 应拒绝显式 NULL");
}

/// 标的 market 检查约束闭集（issue #692 / ADR-0081）：既有 sh/sz/hk/unknown
/// 行为不变，美股三市场 nasdaq/nyse/amex 可落库；闭集外取值仍被 CHECK 拒绝。
/// 存量库不重跑本迁移、保持旧闭集，为已接受的 BREAKING 结论（V002 头部就地
/// 修改注记与 CHANGELOG「Unreleased」BREAKING 条目两级标记）。
#[test]
fn instruments_market_check_accepts_us_markets() {
    let conn = crate::test_support::open();

    for (i, market) in ["sh", "sz", "hk", "nasdaq", "nyse", "amex", "unknown"]
        .into_iter()
        .enumerate()
    {
        probe_instrument_market(&conn, &format!("inst-{i}"), &format!("SYM{i}"), market)
            .unwrap_or_else(|e| panic!("market {market} 应可落库: {e}"));
    }

    // 闭集外取值仍被 CHECK 拒绝。
    let rejected = probe_instrument_market(&conn, "inst-x", "LSE", "lse");
    assert!(rejected.is_err(), "闭集外 market 应被 CHECK 拒绝");
}

/// fx_rate_history：币种对 × 周唯一（与 PriceHistory 同规则）。
#[test]
fn fx_rate_history_weekly_unique_per_pair() {
    let conn = crate::test_support::open();

    seed_fx_rate_history(&conn, "fx-01", "HKD", "CNY", "2026-05-27", 0.92);
    // 同币种对同采样日第二行应被周唯一约束拒绝（探针直写，自定义 id）。
    let dup = probe_fx_rate_history(&conn, "fx-02", "HKD", "CNY", "2026-05-27", 0.92);
    assert!(dup.is_err(), "同币种对同采样日第二行应违反周唯一约束");
    // 同周不同采样日同样拒绝——周采样语义与 PriceHistory 对齐。
    let dup_same_week = probe_fx_rate_history(&conn, "fx-02b", "HKD", "CNY", "2026-05-29", 0.93);
    assert!(dup_same_week.is_err(), "同币种对同周第二行应违反周唯一约束");
    // 不同周可写入；反向币种对是另一条序列，互不冲突。
    seed_fx_rate_history(&conn, "fx-03", "HKD", "CNY", "2026-06-03", 0.92);
    seed_fx_rate_history(&conn, "fx-04", "CNY", "HKD", "2026-05-27", 1.087);
}

// ---------------------------------------------------------------------------
// 全库外键显式 ON DELETE：迁移审计 + 定时交易系删除行为抽查（issue #273 / spec #271）
// ---------------------------------------------------------------------------

/// 插入一条定时交易（最小合法行，category_id 可空由调用方决定；簿记戳引用
/// 工厂固定时刻常量，ADR-0084 决策 5）。
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

/// 插入一条期次（最小合法行，transaction_id 可空由调用方决定）。
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

/// 准备行为抽查的世界：迁移后的内存库 + 一个现金账户；返回连接。
fn world_with_account() -> Connection {
    let conn = crate::test_support::open();
    seed_account(&conn, "acc-01", "现金", "cash", "CNY", 0);
    conn
}

/// 准备行为抽查的世界：一个账户 + 三张定时交易（每 kind 一张），
/// 各带一条期次与对应扩展表行；返回连接。
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

/// 迁移审计不变量：跑完整迁移链后，全库每个外键都必须显式声明 ON DELETE
/// 动作（RESTRICT / CASCADE / SET NULL），不允许 SQLite 默认的 NO ACTION，
/// 无白名单——新增迁移漏写显式 ON DELETE 时本测试确定性失败。
/// 经 `PRAGMA foreign_key_list` 反射观察 schema，不测实现细节。
#[test]
fn migration_audit_every_foreign_key_has_explicit_on_delete() {
    let conn = crate::test_support::open();

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

    // PRAGMA foreign_key_list 列序固定：id, seq, table, from, to, on_update, on_delete, match。
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
    // 覆盖下界只防 PRAGMA 反射静默失效（当前全库 40 条外键，反射失效时应远小于此）；
    // 不用精确计数，避免未来增删外键时本测试产生无关维护点。
    assert!(
        checked >= 30,
        "审计应覆盖全库全部外键，实际仅反射到 {checked} 条"
    );
}

/// 行为抽查 1：硬删分类 → 定时交易的分类引用被置空（SET NULL）、本体保留。
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

/// 行为抽查 2：硬删定时交易 → 期次与三个扩展表（分期/订阅/定时转账）行级联消失（CASCADE）。
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

/// 对照组：定时交易引用的账户硬删被 RESTRICT 拒绝（强依赖不可悬空）。
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
