use std::path::Path;
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
            M::up(include_str!("../migrations/V002__investment.sql")),
            M::up(include_str!("../migrations/V003__seed_defaults.sql")),
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

/// 生成新的 UUID v7（时间有序，适合主键与同步）。
pub fn new_uuid() -> String {
    uuid::Uuid::new_v7(uuid::Timestamp::now(uuid::NoContext)).to_string()
}

/// 当前设备标识。MVP 阶段使用固定占位值，后续可改为从配置文件读取。
pub fn device_id() -> String {
    String::from("device-1")
}

/// 打开数据库连接并启用外键约束（SQLite 默认关闭，需每次连接显式开启）。
/// 所有数据库连接都应通过此函数或其派生函数创建，以保证外键生效。
pub fn open_connection<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(conn)
}

/// 打开内存数据库连接并启用外键约束（用于测试）。
#[cfg(test)]
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    Ok(conn)
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

    /// refund 交易的 schema 与余额/月度聚合：退款继承原交易账户与分类，
    /// 计入账户余额（+退款），月度报表单独列退款，并记录关联原交易 id。
    #[test]
    fn refund_transaction_schema_and_aggregation() {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();

        let account_id = "acc-test-01";
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'现金','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        let cat_id: String = conn
            .query_row(
                "SELECT id FROM categories WHERE name='外卖' AND kind='expense'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // 一笔支出 100 元
        let expense_id = "txn-test-expense";
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id, \
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'expense',10000,'CNY',10000,?2,NULL,?3,NULL,'外卖','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
            params![expense_id, account_id, cat_id],
        )
        .unwrap();

        // 退款 30 元，关联原支出，继承原账户与分类
        let refund_id = "txn-test-refund";
        conn.execute(
            "INSERT INTO transactions \
             (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id, \
             category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'refund',3000,'CNY',3000,?2,NULL,?3,?4,'退部分','2026-01-12','2026-01-12T00:00:00Z','2026-01-12T00:00:00Z',1,'test',0)",
            params![refund_id, account_id, cat_id, expense_id],
        )
        .unwrap();

        // 余额 = 0 + 0 - 10000 + 0 - 0 + 3000 = -7000
        let balance: i64 = conn
            .query_row(
                "SELECT \
                   (SELECT initial_balance_cents FROM accounts WHERE id=?1) \
                 + COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='income' AND is_deleted=0),0) \
                 - COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='expense' AND is_deleted=0),0) \
                 + COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE to_account_id=?1 AND kind='transfer' AND is_deleted=0),0) \
                 - COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='transfer' AND is_deleted=0),0) \
                 + COALESCE((SELECT SUM(amount_native_cents) FROM transactions WHERE account_id=?1 AND kind='refund' AND is_deleted=0),0)",
                params![account_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(balance, -7000);

        // 月度退款列
        let refund: i64 = conn
            .query_row(
                "SELECT SUM(CASE WHEN kind='refund' THEN amount_native_cents ELSE 0 END) \
                 FROM transactions WHERE substr(date,1,7)='2026-01' AND is_deleted=0",
                [],
                |r| r.get::<_, Option<i64>>(0).map(|o| o.unwrap_or(0)),
            )
            .unwrap();
        assert_eq!(refund, 3000);

        // 退款关联原交易
        let linked: String = conn
            .query_row(
                "SELECT refund_of_transaction_id FROM transactions WHERE kind='refund'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(linked, expense_id);
    }

    /// 汇率表支持按日期生效，查询时取 priced_at <= 目标日期的最新汇率。
    #[test]
    fn exchange_rate_with_priced_at() {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();

        conn.execute(
            "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
             VALUES (?1,'USD','CNY',7.0,'2026-01-01','manual','2026-01-01T00:00:00Z',1,'test')",
            params!["er-01"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
             VALUES (?1,'USD','CNY',7.2,'2026-06-01','manual','2026-06-01T00:00:00Z',1,'test')",
            params!["er-02"],
        )
        .unwrap();

        let rate = crate::commands::exchange_rate_for_date(&conn, "USD", "CNY", "2026-01-15")
            .unwrap();
        assert!((rate - 7.0).abs() < 0.0001);

        let rate = crate::commands::exchange_rate_for_date(&conn, "USD", "CNY", "2026-07-01")
            .unwrap();
        assert!((rate - 7.2).abs() < 0.0001);
    }

    /// market_prices 写入最新价后，v_holdings 能正确输出市值和未实现盈亏。
    #[test]
    fn market_price_and_holding_view() {
        let mut conn = open_in_memory().unwrap();
        init_db(&mut conn).unwrap();

        let account_id = "acc-test-inv";
        let instrument_id = "inst-test-nvda";
        conn.execute(
            "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'美股','investment','USD',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
            params![account_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'NVDA','stock','NVIDIA','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();

        let buy_txn_id = "txn-buy-01";
        conn.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'buy',100000,'USD',100000,?2,NULL,NULL,NULL,'买 NVDA','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
            params![buy_txn_id, account_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
             VALUES (?1,?2,'buy',10,10000,0)",
            params![buy_txn_id, instrument_id],
        )
        .unwrap();
        let lot_id = "lot-01";
        conn.execute(
            "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,10,10,10000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
            params![lot_id, account_id, instrument_id, buy_txn_id],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,12000,'USD','2026-07-07','yahoo','2026-07-07T00:00:00Z','2026-07-07T00:00:00Z',1,'test')",
            params!["mp-01", instrument_id],
        )
        .unwrap();

        let (quantity, cost_basis, market_value, unrealized_pnl): (f64, i64, i64, i64) = conn
            .query_row(
                "SELECT quantity, cost_basis_cents, market_value_cents, unrealized_pnl_cents \
                 FROM v_holdings WHERE id=?1",
                params![format!("{account_id}-{instrument_id}")],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!((quantity - 10.0).abs() < 0.0001);
        assert_eq!(cost_basis, 100000);
        assert_eq!(market_value, 120000);
        assert_eq!(unrealized_pnl, 20000);
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

        let native =
            crate::commands::convert_to_native(&conn, 10000, "USD", account_id, "2026-01-10")
                .unwrap();
        assert_eq!(native, 72000);

        // 同币种无需汇率，1:1 返回。
        let native =
            crate::commands::convert_to_native(&conn, 10000, "CNY", account_id, "2026-01-10")
                .unwrap();
        assert_eq!(native, 10000);
    }
}
