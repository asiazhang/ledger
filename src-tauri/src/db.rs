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

    /// 校验迁移集合本身定义正确（在临时内存 DB 上从首到尾跑一遍向上迁移）。
    #[test]
    fn migrations_validate() {
        assert!(migrations().validate().is_ok());
    }

    /// init_db 应幂等：连续跑两次不报错，且默认币种 3 条、分类 12 条已写入。
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
        assert_eq!(cat_count, 12);
    }
}
