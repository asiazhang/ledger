use rusqlite::params;

use super::{
    assert_balance_cache_matches_realtime, open, open_file_scratch, seed_account,
    seed_exchange_rate, seed_fund_market_price, seed_market_price,
};
use super::{seed_fx_rate_history, seed_instrument, seed_investment_setup, seed_price_history};
use ledger_accounts::balance::refresh_account_balances;

fn scalar(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn open_initializes_memory_db() {
    let conn = open();
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "外键约束应随建库打开");
    assert!(
        scalar(&conn, "SELECT COUNT(*) FROM accounts") > 0,
        "迁移默认种子（V004）应在位"
    );
}

#[test]
fn seed_account_lands_full_row() {
    let conn = open();
    seed_account(&conn, "acc-1", "钱包", "cash", "CNY", 6667);
    let (name, kind, currency, initial, created_at, device): (
        String,
        String,
        String,
        i64,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT name,type,currency_code,initial_balance_cents,created_at,device_id \
             FROM accounts WHERE id='acc-1'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        (name.as_str(), kind.as_str(), currency.as_str()),
        ("钱包", "cash", "CNY")
    );
    assert_eq!(initial, 6667);
    assert_eq!(created_at, super::FIXED_NOW, "簿记戳应由工厂内部发放");
    assert_eq!(device, "test");
}

#[test]
fn reference_data_seeds_land_rows() {
    let conn = open();
    seed_instrument(&conn, "inst-1", "600519.SH", "贵州茅台", "CNY", "sh");
    let (symbol, market): (String, String) = conn
        .query_row(
            "SELECT symbol,market FROM instruments WHERE id='inst-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((symbol.as_str(), market.as_str()), ("600519.SH", "sh"));

    seed_price_history(&conn, "ph-1", "inst-1", "2026-05-27", 170000, "CNY");
    let (price, date): (i64, String) = conn
        .query_row(
            "SELECT price_cents,trade_date FROM price_history WHERE id='ph-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((price, date.as_str()), (170000, "2026-05-27"));

    seed_fx_rate_history(&conn, "fxh-1", "HKD", "CNY", "2026-05-27", 0.92);
    let rate: f64 = conn
        .query_row(
            "SELECT rate FROM fx_rate_history WHERE id='fxh-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rate - 0.92).abs() < f64::EPSILON);
}

#[test]
fn seed_exchange_rate_derives_pair_id() {
    let conn = open();
    let id = seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(id, "er-USD-CNY");
    let rate: f64 = conn
        .query_row(
            "SELECT rate FROM exchange_rates WHERE id='er-USD-CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rate - 7.2).abs() < f64::EPSILON);
}

#[test]
fn market_price_seed_lands_single_row() {
    let conn = open();
    seed_instrument(&conn, "inst-1", "600519.SH", "贵州茅台", "CNY", "sh");
    let returned = seed_market_price(&conn, "inst-1", 1_200_000, "CNY");
    assert_eq!(returned, "inst-1", "返回实体 id（登记处形状）");
    let (price, ccy, priced_at, source, created_at, version, device): (
        i64,
        String,
        String,
        Option<String>,
        String,
        i64,
        String,
    ) = conn
        .query_row(
            "SELECT price_cents,currency_code,priced_at,source,created_at,version,device_id \
             FROM market_prices WHERE instrument_id='inst-1'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(price, 1_200_000);
    assert_eq!(ccy, "CNY");
    assert_eq!(priced_at, super::FIXED_NOW, "行情采集戳由工厂内部发放");
    assert_eq!(source, None, "缺 source 即 NULL（现存形状）");
    assert_eq!(created_at, super::FIXED_NOW, "簿记戳由工厂内部发放");
    assert_eq!(version, 1);
    assert_eq!(device, "test");
}

#[test]
fn fund_market_price_seed_lands_nav_watermark() {
    let conn = open();
    seed_instrument(&conn, "inst-fund", "110022", "某基金", "CNY", "unknown");
    seed_fund_market_price(
        &conn,
        "inst-fund",
        33_480,
        "CNY",
        "2026-01-30",
        Some("eastmoney"),
    );
    let (price, priced_at, nav_date, source, created_at): (
        i64,
        String,
        String,
        Option<String>,
        String,
    ) = conn
        .query_row(
            "SELECT price_cents,priced_at,nav_date,source,created_at \
             FROM market_prices WHERE instrument_id='inst-fund'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(price, 33_480);
    assert_eq!(
        priced_at, "2026-01-30",
        "行情日期 = 净值日期（基金现价同形）"
    );
    assert_eq!(nav_date, "2026-01-30", "nav_date 是显式域时刻");
    assert_eq!(source.as_deref(), Some("eastmoney"), "source 透传");
    assert_eq!(created_at, super::FIXED_NOW, "簿记戳由工厂内部发放");
}

#[test]
fn investment_setup_seeds_account_instrument_and_rate() {
    let conn = open();
    let (acc, inst) = seed_investment_setup(&conn, "acc-inv", "inst-k");
    assert_eq!((acc.as_str(), inst.as_str()), ("acc-inv", "inst-k"));
    assert_eq!(
        scalar(
            &conn,
            "SELECT COUNT(*) FROM accounts WHERE id='acc-inv' AND type='investment'"
        ),
        1
    );
    assert_eq!(
        scalar(
            &conn,
            "SELECT COUNT(*) FROM instruments WHERE id='inst-k' AND currency_code='USD'"
        ),
        1
    );
    let rate: f64 = conn
        .query_row(
            "SELECT rate FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rate - 1.0).abs() < f64::EPSILON);
}

#[test]
fn balance_cache_assertion_passes_on_consistent_world() {
    let conn = open();
    seed_account(&conn, "acc-a", "现金", "cash", "CNY", 6667);
    seed_investment_setup(&conn, "acc-inv", "inst-k");
    refresh_account_balances(&conn, &["acc-a", "acc-inv"]).unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

#[test]
#[should_panic(expected = "缓存应等于实时计算")]
fn balance_cache_assertion_catches_drift() {
    let conn = open();
    seed_account(&conn, "acc-a", "现金", "cash", "CNY", 1000);
    refresh_account_balances(&conn, &["acc-a"]).unwrap();
    conn.execute(
        "UPDATE account_balance_cache SET balance_cents = balance_cents + 100 WHERE account_id=?1",
        params!["acc-a"],
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

#[test]
fn open_file_scratch_initializes_like_memory_template() {
    let (conn, dir) = open_file_scratch("selftest-file-tpl");
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "外键约束应随建库打开");
    assert!(
        dir.join(ledger_infra::db::data_location::DB_FILE_NAME)
            .exists(),
        "库文件应落在自带的暂存目录内"
    );
    let memory = open();
    assert_eq!(
        scalar(&conn, "PRAGMA user_version"),
        scalar(&memory, "PRAGMA user_version"),
        "文件库模板应迁移至与内存模板相同的最新版本"
    );
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM accounts"),
        scalar(&memory, "SELECT COUNT(*) FROM accounts"),
        "默认种子应与内存模板等量在位"
    );
    seed_account(&conn, "acc-file", "现金", "cash", "CNY", 0);
    assert_eq!(
        scalar(&conn, "SELECT COUNT(*) FROM accounts WHERE id='acc-file'"),
        1,
        "种子应可在文件库模板连接上注入"
    );
}

#[test]
fn open_file_scratch_cleans_up_scratch_dir_after_test() {
    let dir_path = {
        let (conn, dir) = open_file_scratch("selftest-file-tpl-cleanup");
        let path = dir.path().to_path_buf();
        assert!(
            path.join(ledger_infra::db::data_location::DB_FILE_NAME)
                .exists()
        );
        drop(conn);
        path
    };
    assert!(!dir_path.exists(), "guard drop 后整棵暂存目录应消失");
}

#[test]
fn op_write_failure_probe_blocks_and_unblocks() {
    let conn = open();
    super::block_op_writes(&conn);
    let err = conn
        .execute("INSERT INTO sync.sync_ops DEFAULT VALUES", [])
        .unwrap_err();
    assert!(
        err.to_string().contains("测试注入：op 写失败"),
        "臂装后 INSERT 应被注入触发器挡下：{err}"
    );

    super::unblock_op_writes(&conn);
    let err = conn
        .execute("INSERT INTO sync.sync_ops DEFAULT VALUES", [])
        .unwrap_err();
    assert!(
        !err.to_string().contains("测试注入：op 写失败"),
        "解除后同语句不应再命中注入（应为约束错误）：{err}"
    );
}
