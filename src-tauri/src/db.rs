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

    /// 汇率表每货币对仅保留一行最新；exchange_rate 按 (base_code, quote_code) 直查。
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

        let rate = crate::commands::exchange_rate(&conn, "USD", "CNY").unwrap();
        assert!((rate - 7.2).abs() < 0.0001);

        // 同货币对第二行应被 UNIQUE(base_code, quote_code) 拒绝。
        let dup = conn.execute(
            "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
             VALUES (?1,'USD','CNY',7.0,'2026-01-01','manual','2026-01-01T00:00:00Z',1,'test')",
            params!["er-02"],
        );
        assert!(dup.is_err(), "同货币对第二行应违反唯一约束");

        // 反向兌底：CNY->USD 未直接录入，但 USD->CNY 存在，应返回 1/7.2。
        let rev = crate::commands::exchange_rate(&conn, "CNY", "USD").unwrap();
        assert!((rev - 1.0 / 7.2).abs() < 0.0001, "反向汇率应为 1/7.2: {rev}");

        // 正反向均未录入的货币对才返回错误。
        assert!(crate::commands::exchange_rate(&conn, "EUR", "JPY").is_err());

        // 同币种直返回 1.0，无需查表。
        assert!((crate::commands::exchange_rate(&conn, "USD", "USD").unwrap() - 1.0).abs() < 0.0001);
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
                params![format!("{account_id}-{instrument_id}-USD")],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert!((quantity - 10.0).abs() < 0.0001);
        assert_eq!(cost_basis, 100000);
        assert_eq!(market_value, 120000);
        assert_eq!(unrealized_pnl, 20000);
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
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'NVDA','stock','NVIDIA','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
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
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'REV','stock','reverse test','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
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
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'MULTI','stock','多币种标的','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
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
            "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,'SOFT','stock','soft-delete test','USD','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
            params![instrument_id],
        )
        .unwrap();

        // 两个账户各一笔买入 + lot
        for (acc, txn, lot) in [(active_acc, "txn-soft-a", "lot-soft-a"), (deleted_acc, "txn-soft-d", "lot-soft-d")] {
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
        assert_eq!(account_ids, vec![active_acc.to_string()], "软删除账户的持仓不应出现在视图");
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
        assert_eq!(old, 0, "旧冗余索引 idx_security_lots_account_instrument 应已删除");

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

        let native =
            crate::commands::convert_to_native(&conn, 10000, "USD", account_id)
                .unwrap();
        assert_eq!(native, 72000);

        // 同币种无需汇率，1:1 返回。
        let native =
            crate::commands::convert_to_native(&conn, 10000, "CNY", account_id)
                .unwrap();
        assert_eq!(native, 10000);
    }
}
