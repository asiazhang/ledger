//! 价格写入单点（ADR-0036 / ADR-0038 / ADR-0019）：现价缓存 upsert、价格历史
//! 周采样 upsert 与价格刻度换算。投资域全部价格写入通道（行情同步 / 增量同步 /
//! 净值同步 / 手动报价 / 现价录入命令 / 基金接入）共用本模块，不另写第二份
//! upsert SQL（issue #291 收口）。行情同步引擎（`crate::sync` 域，#407 归位）
//! 作为价格消费通道经本模块落库——域间横向依赖（ADR-0056 决策 2 允许）。
//!
//! 置脏触发已收口连接层统一写入口（`db::write_locked`，ADR-0032）：本模块对备份域
//! 零感知，写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use rusqlite::{Connection, params};

use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::Result;
use ledger_sync_protocol::device::device_id;

/// 价格刻度换算因子（ADR-0038）：1 分 = 100 万分之一元——
/// 金额（分）= 数量 × 单价（万分之一元）÷ 本因子；手续费分摊薄入每份成本时
/// 乘本因子归到价格刻度。与 `v_holdings` 视图表达式（V002）同口径，视图 SQL
/// 无法引用 Rust 常量，两侧以本词条注释互认，改其一必同步另一。
pub const PRICE_UNITS_PER_FEN: f64 = 100.0;

/// 同步价格数据来源标记常量：价格侧 source 词表与字典侧同词（ADR-0036），
/// 与手动报价的 [`super::manual_price::MANUAL_PRICE_SOURCE`] 对称。
///
/// 存量行仍为 `eastmoney`（历史事实，不重写不迁移）；换源后新写入的来源标记
/// 按实际取数源取值——场内现价走腾讯（[`TENCENT_PRICE_SOURCE`]）、场外基金
/// 净值走新浪（[`SINA_PRICE_SOURCE`]）。
pub const EASTMONEY_PRICE_SOURCE: &str = "eastmoney";

/// 场内（沪深港美股票与场内基金）价格来源标记（ADR-0130 决策 7）：行情同步的
/// 场内通道改走腾讯行情报价后写入（现价缓存、当周采样点与历史补全共用）。
/// 价格侧无 CHECK，闭集由写入通道收口。
pub const TENCENT_PRICE_SOURCE: &str = "tencent";

/// 场外基金净值来源标记（ADR-0130 决策 7 / issue #1565）：行情同步的基金净值
/// 通道改走新浪后写入——批量面直落的现价与当周采样点（issue #1565 起），单只面
/// 补数随 #1566 接入。价格侧无 CHECK，闭集由写入通道收口。
pub const SINA_PRICE_SOURCE: &str = "sina";

/// 证监会基金电子披露来源标记（ADR-0130 决策 7 / issue #1568）：基金按代码
/// 查询与创建的已终止基金兜底面取价（最后一期单位净值），以及经官方自报形态
/// 确认的建档常量价（恒定单位净值 1.0000 的确认源，ADR-0126 决策 3）。价格侧
/// 无 CHECK，闭集由写入通道收口。
pub const CSRC_PRICE_SOURCE: &str = "csrc";

/// 真实价格值（元）→ 万分之一元（0.0001 元，价格刻度 ADR-0038）。
/// A 股/港股 K 线收盘价与场外基金单位净值同刻度换算（基金净值 4 位小数，
/// issue #301），统一 ×10000。
pub fn price_value_to_cents(value: f64) -> i64 {
    (value * 10000.0).round() as i64
}

/// 按 (标的, ISO 周) 插入或覆盖一条周采样价格历史（issue #137 / ADR-0019）。
/// 「整周覆盖」幂等由 UNIQUE(instrument_id, week_start)（week_start 为生成列）保证：
/// 同周任一采样日写入都落在同一行上，重复回填零重复行。清仓不删历史（仅随标的删除级联）。
/// `source` 为价格数据来源标记（与字典侧 source 同词表）：同步按实际取数源取值、手动报价 'manual'
/// （ADR-0036）——周采样落库单点，不立第二承载。
pub fn upsert_price_history(
    conn: &Connection,
    instrument_id: &str,
    trade_date: &str,
    price_cents: i64,
    currency: &str,
    source: &str,
) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO price_history (id,instrument_id,trade_date,price_cents,currency_code,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?7,1,?8) \
         ON CONFLICT(instrument_id, week_start) DO UPDATE SET \
         trade_date=excluded.trade_date, price_cents=excluded.price_cents, \
         currency_code=excluded.currency_code, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![new_uuid(), instrument_id, trade_date, price_cents, currency, source, now, device_id(conn)?],
    )?;
    Ok(())
}

/// 现价缓存写入输入（ADR-0103 决策 4）：conn + 6 个业务位置参数收敛为单一输入
/// 结构，不搞 builder。通道语义（股票 `priced_at` = 写入时刻、基金 = 净值日期、
/// 净值水位比较、最新点映像）留在各自通道，不收进本结构。
#[derive(Debug, Clone, Copy)]
pub struct MarketPriceWrite<'a> {
    /// 标的 id（现价行按 instrument_id 唯一，未命中即新建）。
    pub instrument_id: &'a str,
    /// 价格（万分之一元，ADR-0038 价格刻度）。
    pub price_cents: i64,
    /// 报价币种（随标的字典）。
    pub currency_code: &'a str,
    /// 该价格对应的行情 / 净值日期（由调用通道判定）。
    pub priced_at: &'a str,
    /// 单位净值日期（兼任净值同步水位，ADR-0038）：仅场外基金现价携带；
    /// 股票与手动报价传 None（无净值日期语义，覆盖为 NULL）。
    pub nav_date: Option<&'a str>,
    /// 价格数据来源（与字典侧 source 同词表）：同步按实际取数源取值、手动报价 'manual'
    ///（ADR-0036）；可空透传是已发布行为（重放与 `create_market_price` 透传）。
    pub source: Option<&'a str>,
}

/// 按 instrument_id 插入或更新一条行情价格——现价缓存写入的单点（投资域旧半成品
/// `crud::create_market_price` 已委托至此，issue #291 收口），不另写第二份 upsert SQL。
pub fn upsert_market_price(conn: &Connection, input: &MarketPriceWrite<'_>) -> Result<String> {
    let MarketPriceWrite {
        instrument_id,
        price_cents,
        currency_code,
        priced_at,
        nav_date,
        source,
    } = *input;
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM market_prices WHERE instrument_id=?1",
            params![instrument_id],
            |r| r.get(0),
        )
        .ok();
    let id = existing_id.unwrap_or_else(new_uuid);
    let now = now_iso();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,nav_date,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
         ON CONFLICT(instrument_id) DO UPDATE SET \
         price_cents=excluded.price_cents, currency_code=excluded.currency_code, \
         priced_at=excluded.priced_at, nav_date=excluded.nav_date, source=excluded.source, \
         updated_at=excluded.updated_at, version=version+1",
        params![
            id,
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            nav_date,
            source,
            now,
            now,
            1,
            device_id(conn)?
        ],
    )?;
    Ok(id)
}
