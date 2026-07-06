use std::sync::OnceLock;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::error::Result;

/// 迁移集合。新增 schema 变更或种子数据时，在 `src-tauri/migrations/` 下新建
/// `V00X__名称.sql`，并在 `migrations()` 的 `vec!` 里追加
/// `M::up(include_str!("../migrations/V00X__名称.sql"))`。
/// 版本由 SQLite 的 `user_version` 字段自动追踪，无需手动维护版本表。
fn migrations() -> &'static Migrations<'static> {
    static MIGRATIONS: OnceLock<Migrations<'static>> = OnceLock::new();
    MIGRATIONS.get_or_init(|| {
        Migrations::new(vec![
            M::up(include_str!("../migrations/V001__initial.sql")),
            M::up(include_str!("../migrations/V002__seed_defaults.sql")),
        ])
    })
}

/// 初始化数据库 schema 与默认种子数据（全部由迁移驱动）。
pub fn init_db(conn: &mut Connection) -> Result<()> {
    migrations().to_latest(conn)?;
    Ok(())
}

/// 当前 UTC 时间 ISO 字符串。
pub fn now_iso() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    /// 校验迁移集合本身定义正确（在临时内存 DB 上从首到尾跑一遍向上迁移）。
    #[test]
    fn migrations_validate() {
        assert!(migrations().validate().is_ok());
    }

    /// init_db 应幂等：连续跑两次不报错，且默认币种 3 条、分类 89 条已写入
    /// （18 顶级 + 71 二级）。
    #[test]
    fn init_db_is_idempotent_and_seeds_defaults() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&mut conn).unwrap();
        init_db(&mut conn).unwrap();

        let currency_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM currencies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(currency_count, 3);

        let cat_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cat_count, 89);

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

    /// refund 交易的 schema 与余额/月度聚合：退款继承原交易账户与分类，
    /// 计入账户余额（+退款），月度报表单独列退款，并记录关联原交易 id。
    #[test]
    fn refund_transaction_schema_and_aggregation() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (name,type,currency_code,initial_balance_cents,created_at) \
             VALUES ('现金','cash','CNY',0,'2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
        let account_id: i64 = conn
            .query_row("SELECT id FROM accounts", [], |r| r.get(0))
            .unwrap();
        let cat_id: i64 = conn
            .query_row(
                "SELECT id FROM categories WHERE name='外卖' AND kind='expense'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // 一笔支出 100 元
        conn.execute(
            "INSERT INTO transactions \
             (kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id, \
             category_id,refund_of_transaction_id,note,date,created_at) \
             VALUES ('expense',10000,'CNY',10000,?1,NULL,?2,NULL,'外卖','2026-01-10','2026-01-10T00:00:00Z')",
            params![account_id, cat_id],
        )
        .unwrap();
        let expense_id: i64 = conn.last_insert_rowid();

        // 退款 30 元，关联原支出，继承原账户与分类
        conn.execute(
            "INSERT INTO transactions \
             (kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id, \
             category_id,refund_of_transaction_id,note,date,created_at) \
             VALUES ('refund',3000,'CNY',3000,?1,NULL,?2,?3,'退部分','2026-01-12','2026-01-12T00:00:00Z')",
            params![account_id, cat_id, expense_id],
        )
        .unwrap();

        // 余额 = 0 + 0 - 10000 + 0 - 0 + 3000 = -7000
        let balance: i64 = conn
            .query_row(
                "SELECT \
                   (SELECT initial_balance_cents FROM accounts WHERE id=?1) \
                 + COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='income'),0) \
                 - COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='expense'),0) \
                 + COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE to_account_id=?1 AND kind='transfer'),0) \
                 - COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='transfer'),0) \
                 + COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='refund'),0)",
                params![account_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(balance, -7000);

        // 月度退款列
        let refund: i64 = conn
            .query_row(
                "SELECT SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
                 FROM transactions WHERE substr(date,1,7)='2026-01'",
                [],
                |r| r.get::<_, Option<i64>>(0).map(|o| o.unwrap_or(0)),
            )
            .unwrap();
        assert_eq!(refund, 3000);

        // 退款关联原交易
        let linked: i64 = conn
            .query_row(
                "SELECT refund_of_transaction_id FROM transactions WHERE kind='refund'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, expense_id);
    }
}
