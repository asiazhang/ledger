//! V017 余额与净资产缓存迁移测试（issue #491 / ADR-0067）：
//! 旧版本备份升级路径下，余额缓存回填值必须与实时计算（`compute_balance`，
//! `account_flow_expr` 单一口径矩阵）逐账户完全一致；净资产缓存表为空行
//! 单例（首次读取由读探针指纹自愈回填）。
//!
//! 迁移是一次性冻结产物，SQL 内回填表达式与 Rust 侧 `account_flow_expr`
//! 的一致性不靠人工比对注释，靠本测试锁定：回填值 == 实时重算。

use rusqlite::{Connection, params};

use crate::db::migrations;
use crate::test_support::{assert_balance_cache_matches_realtime, read_scalar_i64, seed_account};

/// V017 之前的 schema 版本：余额缓存表加入前的迁移序列条数（V001–V016，无 V005，
/// 共 15 条；version 为迁移向量下标从 1 起，V017 本身即 version 16）。
const PRE_V017_SCHEMA_VERSION: usize = 15;

/// 在旧 schema（无缓存表）上构造存量世界：多账户 + 全 kind 交易覆盖
/// （income/refund/sell 为 +，expense/buy 为 −，transfer 双侧，split 恒 0，
/// 含软删行）。`amount_native_cents` 直接取 `amount_cents`（CNY 1:1）。
fn seed_legacy_world(conn: &Connection) {
    // 账户行经工厂种子（归一签名，spec #728 / ADR-0084 决策 4）。
    // 现金 A：初始余额 + income + expense + transfer 转出 + refund。
    seed_account(conn, "acc-a", "现金A", "cash", "CNY", 10000);
    // 现金 B：transfer 转入侧 + 软删行（不计入）。
    seed_account(conn, "acc-b", "现金B", "cash", "CNY", 0);
    // 投资账户：buy/sell 路径。
    seed_account(conn, "acc-inv", "美股", "investment", "USD", 0);
    // 已软删账户：不回填、不参与实时计算（软删语义为本测试场景输入，
    // 工厂种子后 is_deleted 修正）。
    seed_account(conn, "acc-gone", "已删", "cash", "CNY", 0);
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id='acc-gone'", [])
        .unwrap();

    let now = "2026-01-15T00:00:00Z";
    let insert_tx = |id: &str,
                     kind: &str,
                     amount: i64,
                     account_id: &str,
                     to: Option<&str>,
                     deleted: i64| {
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,refund_of_transaction_id,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,?2,?3,'CNY',?3,?4,?5,NULL,'2026-01-15',?6,?6,1,'test',?7)",
            params![id, kind, amount, account_id, to, now, deleted],
        ).unwrap();
    };
    // income + / expense − / transfer 双侧 / refund + / 软删行不计入。
    insert_tx("tx-in", "income", 5000, "acc-a", None, 0);
    insert_tx("tx-ex", "expense", 1200, "acc-a", None, 0);
    insert_tx("tx-tf", "transfer", 800, "acc-a", Some("acc-b"), 0);
    insert_tx("tx-rf", "refund", 200, "acc-a", None, 0);
    insert_tx("tx-del", "income", 99999, "acc-a", None, 1);
    // 投资侧：buy − / sell +（amount_native_cents 为折算后金额）。
    insert_tx("tx-buy", "buy", 3000, "acc-inv", None, 0);
    insert_tx("tx-sell", "sell", 1000, "acc-inv", None, 0);
}

/// V017 升级路径：旧 schema 停在 16 版，init_db 补齐 V017 后余额缓存回填
/// 与实时计算逐账户一致（迁移 SQL 表达式 ↔ Rust account_flow_expr 一致性锁定）。
#[test]
fn v017_backfill_matches_compute_balance_on_upgrade() {
    // 升级路径：先开停在旧 schema 的库、种存量世界，再补齐迁移（建库经 db 测试
    // 域薄皮 `open_at_schema_version`；补齐走产品迁移缝 `to_latest`，即 init_db
    // 的迁移核心——spec #728 / issue #754 / ADR-0084 决策 7）。
    let mut conn = super::common::open_at_schema_version(PRE_V017_SCHEMA_VERSION);
    seed_legacy_world(&conn);

    migrations().to_latest(&mut conn).unwrap();

    // 回填值 == 实时重算：断言体上收共享断言库（issue #751 / ADR-0084 决策 6），
    // 逐账户比对全部未删除账户（含本世界三账户与 V004 默认种子账户、投资与转入
    // 侧），与 transaction 域 balance_cache 测试同一份断言体。
    assert_balance_cache_matches_realtime(&conn);

    // 软删账户同样被回填（迁移一次性冻结产物不过滤 is_deleted；读路径不读它，
    // 审计命令只巡检未删除账户，回填行留存无害）。

    // 期望值抽查（锚定口径，防止两侧同错）：acc-a = 10000 + 5000 − 1200 − 800 + 200。
    let a = read_scalar_i64(
        &conn,
        "SELECT balance_cents FROM account_balance_cache WHERE account_id='acc-a'",
        [],
    )
    .expect("acc-a 应有缓存行");
    assert_eq!(a, 13200, "acc-a 回填值应符合口径锚点");

    // 净资产缓存为空单例：迁移不回填，首次读取由读探针自愈。
    let nw_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM net_worth_cache", [], |r| r.get(0))
        .unwrap();
    assert_eq!(nw_rows, 0, "净资产缓存应由读探针首次读取时回填");
}
