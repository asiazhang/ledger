//! 测试种子：吸收跨 ≥2 域重复的实体列清单与投资铺垫组合（ADR-0084 决策 1/3/4）。
//!
//! 形状纪律：全位置参数签名（调用点显式、可组合）；`id` 由调用方指定
//! （外键引用显式）或函数内部发放（如 [`seed_market_price`] 的行 id），返回实体
//! id；簿记戳（created_at/updated_at）由 [`FIXED_NOW`](super::FIXED_NOW)
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
    seed_exchange_rate_row(conn, base, quote, rate, FIXED_NOW, None)
}

/// 种入一行带来源与采集日期的「当前汇率」（issue #1543：人工行保护的 source
/// 形态）。行 id 与簿记戳同 [`seed_exchange_rate`]；`priced_at` 与 `source`
/// 是被测行为输入，显式传入（source 传 "manual" 即人工行）。
pub fn seed_exchange_rate_with_source(
    conn: &Connection,
    base: &str,
    quote: &str,
    rate: f64,
    priced_at: &str,
    source: &str,
) -> String {
    seed_exchange_rate_row(conn, base, quote, rate, priced_at, Some(source))
}

/// `exchange_rates` 插入同体（吸收各处同形状插入的单一形态；source 不传即 NULL）。
fn seed_exchange_rate_row(
    conn: &Connection,
    base: &str,
    quote: &str,
    rate: f64,
    priced_at: &str,
    source: Option<&str>,
) -> String {
    let id = format!("er-{base}-{quote}");
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,source,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,'test')",
        params![id, base, quote, rate, priced_at, source, FIXED_NOW],
    )
    .unwrap();
    id
}

/// 种入一行标的当前行情（现价缓存单行，`v_holdings` 据此算市值；吸收 investment
/// 域 `holdings_summary` / `cumulative_pnl` / `mwr`（经 common）、`trade`、
/// `instrument_list`、`instrument_delete`，`dashboard`，infra 域测 `holding`，壳层
/// 集成测试 `cross_book_summary` 等处的同体裸插
/// ——ADR-0084 决策 1：≥2 域同体消费即上收）。行 id 由 `new_uuid()` 内部发放；
/// 簿记戳与 `priced_at` 全发 [`FIXED_NOW`](super::FIXED_NOW)（`priced_at` 语义为
/// 行情采集时间、测试不读它，先例 [`seed_exchange_rate`]）；`source` 固定 NULL、
/// `nav_date` 固定 NULL（股票/手动现价形态）、`version` 1、`device_id` 'test'。
/// 带净值水位的基金现价见 [`seed_fund_market_price`]（吸收 `fund_trade` 与壳层
/// `instrument_sync` 两处裸插）。
pub fn seed_market_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
) -> String {
    seed_market_price_row(
        conn,
        instrument_id,
        price_cents,
        currency_code,
        FIXED_NOW,
        None,
        None,
    )
}

/// 种入一行携带净值水位的基金现价（`nav_date` 兼任净值同步水位，ADR-0038；
/// 吸收 `fund_trade` 持仓显形与壳层 `instrument_sync` 降级场景基线两处同体裸插
/// ——ADR-0084 决策 1）。`nav_date` 是域时刻（bulk「无新净值」判据与增量窗口读
/// 它）显式传入；`priced_at` 与 `nav_date` 同值——基金现价行情日期 = 净值日期是
/// 生产写入单点的同形（`upsert_market_price` 调用方）；`source` 是被测行为输入
/// （同步存量行 'eastmoney' 等）显式传入，无源行传 `None`。簿记戳仍发
/// [`FIXED_NOW`](super::FIXED_NOW)，行 id 内部发放。
pub fn seed_fund_market_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
    nav_date: &str,
    source: Option<&str>,
) -> String {
    seed_market_price_row(
        conn,
        instrument_id,
        price_cents,
        currency_code,
        nav_date,
        Some(nav_date),
        source,
    )
}

/// `market_prices` 插入同体（`seed_market_price` 与 [`seed_fund_market_price`] 的
/// 共享体；`priced_at` / `nav_date` / `source` 是两形态的差异输入，簿记戳恒发
/// [`FIXED_NOW`](super::FIXED_NOW)，先例 [`seed_exchange_rate_row`]）。
fn seed_market_price_row(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    currency_code: &str,
    priced_at: &str,
    nav_date: Option<&str>,
    source: Option<&str>,
) -> String {
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8,1,'test')",
        params![
            ledger_infra::db::new_uuid(),
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            nav_date,
            source,
            FIXED_NOW,
        ],
    )
    .unwrap();
    instrument_id.to_string()
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

/// 投资铺垫组合种子：账户 + 标的 + 1:1 汇率一行 + 1:1 周点序列（ADR-0084 决策 4，
/// 吸收 transaction 域 `setup_investment_account` 与 api 集成
/// `seed_investment_account` 的体）。账户为 USD 投资账户、标的为 USD 股票；
/// 汇率 USD→CNY 1:1 当期行服务读路径，1:1 周点序列服务写路径（#1547，见
/// [`seed_fx_history_series_1to1`]; buy/sell 本位币折算走 Amount 接缝，issue #70
/// ——非默认币种账户交易不报缺汇率）。非 1:1 折算是测试的行为输入，不经本种子
/// 表达：调用方可删除该 1:1 行后经 [`seed_exchange_rate`] / [`seed_fx_rate_history`]
/// 种入目标汇率。
pub fn seed_investment_setup(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
) -> (String, String) {
    seed_account(conn, account_id, "美股", "investment", "USD", 0);
    seed_instrument(conn, instrument_id, "SYM", "Symbol", "USD", "unknown");
    seed_exchange_rate(conn, "USD", "CNY", 1.0);
    seed_fx_history_series_1to1(conn, "USD", "CNY");
    (account_id.to_string(), instrument_id.to_string())
}

/// 为给定日期各自所属周种汇率历史周点（#1547 写路径按交易日取数的测试夹具）：
/// 同周去重、`INSERT OR IGNORE` 幂等，不覆盖已有点（读侧语义测试先种的特定
/// 汇率不受影响）。交易写入按所属 ISO 周取数，日期清单由调用方按测试内交易
/// 日期给出，汇率通常与该测试的当期行同值。
pub fn seed_fx_history_weeks(
    conn: &Connection,
    base: &str,
    quote: &str,
    rate: f64,
    dates: &[&str],
) {
    let mut done: Vec<String> = Vec::new();
    for d in dates {
        let week: String = conn
            .query_row("SELECT date(?1,'-6 days','weekday 1')", params![d], |r| {
                r.get(0)
            })
            .unwrap();
        if done.contains(&week) {
            continue;
        }
        done.push(week.clone());
        conn.execute(
            "INSERT OR IGNORE INTO fx_rate_history (id,base_code,quote_code,trade_date,rate,source,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,?6,'eastmoney',?5,?5,1,'test')",
            params![format!("fxh-{base}-{quote}-{week}"), base, quote, week, FIXED_NOW, rate],
        )
        .unwrap();
    }
}

/// 写路径按交易日折算所需的汇率历史周点序列（#1547）：buy/sell/convert/dividend
/// 等写路径按交易所属 ISO 周取 `fx_rate_history`，1:1 周点覆盖测试交易日期的
/// 宽窗口（2025-01-06 起每周一点，共 160 周）；与 1:1 当期行并存——当期表服务
/// 读路径、历史序列服务写路径（#1541 拆分）。`INSERT OR IGNORE`：同一连接内
/// 多次调用 setup 不撞周唯一约束。
fn seed_fx_history_series_1to1(conn: &Connection, base: &str, quote: &str) {
    // 窗口依据：本仓测试交易日期集中在 2026 年（工厂日期 2026-01~02、mwr 到
    // 2027-01），自 2025-01-06 起 160 周覆盖 2025-01 ~ 2028-02，留双倍余量。
    let mut week = "2025-01-06".to_string();
    let mut dates: Vec<String> = Vec::with_capacity(160);
    for _ in 0..160 {
        dates.push(week.clone());
        week = conn
            .query_row("SELECT date(?1,'+7 days')", params![week], |r| {
                r.get::<_, String>(0)
            })
            .unwrap();
    }
    let refs: Vec<&str> = dates.iter().map(String::as_str).collect();
    seed_fx_history_weeks(conn, base, quote, 1.0, &refs);
}
