//! 测试种子：吸收跨 ≥2 域重复的实体列清单与投资铺垫组合（ADR-0084 决策 1/3/4）。
//!
//! 形状纪律：全位置参数签名（调用点显式、可组合），`id` 由调用方指定（外键引用
//! 显式），返回实体 id；簿记戳（created_at/updated_at）由 [`FIXED_NOW`](super::FIXED_NOW)
//! 内部发放，域时刻经参数显式传入（默认值引用常量）。

use rusqlite::{Connection, params};

use super::FIXED_NOW;

/// 种入一个账户行（吸收 transaction/investment 两域 `insert_account` 同体函数，
/// 签名按现存最全形状归一，ADR-0084 决策 4）。`kind` 为账户类型闭集字符串
/// （cash/bank/credit/ewallet/investment/debt/receivable/other），初始余额以分计。
pub fn seed_account(
    conn: &Connection,
    id: &str,
    name: &str,
    kind: &str,
    currency: &str,
    initial_balance_cents: i64,
) -> String {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?6,1,'test',0)",
        params![id, name, kind, currency, initial_balance_cents, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

/// 种入一个股票标的行（吸收 db/investment 两域 `insert_instrument` 同体函数）。
/// 类型固定 stock（基金/手动形态等单域变体留域薄皮）；市场为显式参数（db 域用
/// `sh`、投资域用 `unknown`，两域吸收体并集）。
pub fn seed_instrument(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
) -> String {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'stock',?3,?4,?5,?6,?6,1,'test')",
        params![id, symbol, name, currency, market, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

/// 种入一行「当前汇率」（吸收 investment 域 `insert_rate_1_1` / `insert_rate` 与
/// transaction、api_server 各处同形状 `exchange_rates` 插入）。行 id 由货币对派生
/// （表约束 `UNIQUE(base_code, quote_code)`：每货币对仅一行最新汇率）；簿记戳取
/// [`FIXED_NOW`](super::FIXED_NOW)，`priced_at` 语义为「行情采集时间」，测试不读它。
pub fn seed_exchange_rate(conn: &Connection, base: &str, quote: &str, rate: f64) -> String {
    let id = format!("er-{base}-{quote}");
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?5,1,'test')",
        params![id, base, quote, rate, FIXED_NOW],
    )
    .unwrap();
    id
}

/// 种入一条价格历史周采样点（吸收 db 域 `insert_price_history` 同体函数；投资域
/// trend 测试的显式价格形状为并集上界）。`trade_date` 是域时刻（行为输入），显式
/// 传入；来源固定 eastmoney（周唯一约束见 V010，同周两点会被库层拒绝）。
pub fn seed_price_history(
    conn: &Connection,
    id: &str,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
    currency: &str,
) -> String {
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,'test')",
        params![id, instrument_id, trade_date, price_cents, currency, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

/// 种入一条汇率历史周采样点（吸收 db 域 `insert_fx_rate_history` 同体函数）。
/// `trade_date` 是域时刻（行为输入），显式传入；来源固定 eastmoney。
pub fn seed_fx_rate_history(
    conn: &Connection,
    id: &str,
    base: &str,
    quote: &str,
    trade_date: &str,
    rate: f64,
) -> String {
    conn.execute(
        "INSERT INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,'eastmoney',?6,?6,1,'test')",
        params![id, base, quote, trade_date, rate, FIXED_NOW],
    )
    .unwrap();
    id.to_string()
}

/// 投资铺垫组合种子：账户 + 标的 + 1:1 汇率一行建成（ADR-0084 决策 4，吸收
/// transaction 域 `setup_investment_account` 与 api 集成 `seed_investment_account`
/// 的体）。账户为 USD 投资账户、标的为 USD 股票、汇率 USD→CNY 1:1（buy/sell 本位币
/// 折算走 Amount 接缝，issue #70——非默认币种账户交易不报缺汇率）。非 1:1 折算是
/// 测试的行为输入，不经本种子表达：表约束每货币对仅一行，调用方可删除该 1:1 行
/// 后经 [`seed_exchange_rate`] 种入目标汇率。
pub fn seed_investment_setup(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
) -> (String, String) {
    seed_account(conn, account_id, "美股", "investment", "USD", 0);
    seed_instrument(conn, instrument_id, "SYM", "Symbol", "USD", "unknown");
    seed_exchange_rate(conn, "USD", "CNY", 1.0);
    (account_id.to_string(), instrument_id.to_string())
}
