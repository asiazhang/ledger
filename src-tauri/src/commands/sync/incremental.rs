//! 持仓价格增量同步编排（issue #103，issue #137 升级，issue #303 基金分区；标的
//! 收集改走单点谓词 issue #239）：单次收集全量持仓标的（口径 = InvestedInstrument，
//! 见 `investment::predicates`），一次执行完成四件事：① 批量报价刷股票现价 upsert
//! `market_prices`；② 每股票一次日 K 请求回填近两年日线，本地降采样为周线落
//! `price_history`；③ 持仓非本位币币种对的汇率 K 线同期落 `fx_rate_history`；
//! ④ 基金走历史净值通道逐只按水位增量回填（ADR-0038 决策 6，见 `fund_nav`）。
//! 类型分区在 Rust 侧完成，不增删、不改标的字典（名称/市场/数量）。
//! 职责切分：全量同步修字典 / 增量同步刷价格 + 沉淀历史（issue #101、#137）。
//!
//! 编排与网络解耦：核心流程 [`do_incremental_sync_with`] 接受注入的批量报价 / 日 K /
//! 汇率 K 三个闭包（同一签名 `&str → Result<Vec<_>>`）与历史净值页闭包
//!（[`NavQuery`] → [`LsjzPage`]），测试以 mock 数据驱动（不依赖真实网络）；生产经
//! [`do_incremental_sync`] 接 HTTP 层（复用主机池/重试/限流 pacer 与价格换算）。

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use crate::error::Result;
use crate::investment::predicates::INVESTED_EXISTS;
use crate::investment::prices::{
    EASTMONEY_PRICE_SOURCE, price_value_to_cents, upsert_market_price, upsert_price_history,
};
use crate::models::SyncHoldingPricesResult;
use crate::transaction::amount::default_currency_code;

use super::fund_nav::{LsjzPage, NavQuery, sync_fund_navs};
use super::http::{
    KlineBar, Pacer, StockItem, ULIST_BATCH_SIZE, build_client, f2_to_price, fetch_fx_kline,
    fetch_kline, fetch_ulist, secid_prefix,
};
use super::persist::upsert_fx_rate_history;

/// 持仓股票的报价代码：东财 secid 与响应 f12 均为裸代码（如 600519 / 00700）。
/// 字典 symbol 可能带市场后缀（schema 注释示例格式如 "600519.SH"），取点号前段归一化。
fn quote_code(symbol: &str) -> &str {
    symbol.split('.').next().unwrap_or(symbol)
}

/// 持仓标的信息（一个标的一条，全量标的集合，跨账户去重由单表驱动天然成立）。
pub(super) struct HeldInstrument {
    pub(super) instrument_id: String,
    pub(super) symbol: String,
    pub(super) market: String,
    pub(super) currency: String,
    pub(super) instrument_type: String,
}

impl HeldInstrument {
    /// 是否股票类：走行情报价/日 K 通道。
    fn is_stock(&self) -> bool {
        self.instrument_type == "stock"
    }

    /// 是否场外基金：走历史净值通道（ADR-0038 决策 6）。
    fn is_fund(&self) -> bool {
        self.instrument_type == "fund"
    }
}

/// 单次收集全量持仓标的（一条 SQL、一个谓词引用点）：口径走投资域单点谓词
/// [`INVESTED_EXISTS`]（InvestedInstrument，ADR-0015 决策 1；别名契约
/// i = instruments），与 invested 派生列、「只看持仓」过滤、持仓概览同源，
/// 不再读 v_holdings 视图（视图与谓词的一致性由投资域绑定测试钉住）。
/// 按 symbol 升序；股票/非股票分区在 Rust 侧完成（见 [`do_incremental_sync_with`]）。
fn collect_held_instruments(conn: &Connection) -> Result<Vec<HeldInstrument>> {
    let sql = format!(
        "SELECT i.id, i.symbol, i.market, i.currency_code, i.instrument_type \
         FROM instruments i \
         WHERE {INVESTED_EXISTS} \
         ORDER BY i.symbol"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(HeldInstrument {
            instrument_id: r.get(0)?,
            symbol: r.get(1)?,
            market: r.get(2)?,
            currency: r.get(3)?,
            instrument_type: r.get(4)?,
        })
    })?;
    let mut instruments = Vec::new();
    for row in rows {
        instruments.push(row?);
    }
    Ok(instruments)
}

/// 增量同步核心流程：单次收集全量持仓标的并按类型分区 → 股票侧构造 secid
/// 批量报价 upsert 现价 + 日 K 回填周线；基金侧逐只历史净值按水位增量回填
///（ADR-0038 决策 6，委托 [`sync_fund_navs`]）；汇率 K 线同期落
/// `fx_rate_history` → 结果统计。
/// 四个抓取函数均由调用方注入（生产接 HTTP 层，测试注入 mock），本函数不触碰网络。
/// 返回统计：`synced` = 处理成功的标的数（股票有效价 + 基金处理成功，含基金
/// 「已是最新」）；`skipped` = 债券等无行情来源持仓 + 名称充代码的基金行 + 市场未知
/// + 停牌/无效价/查询无果 + 首刷查无净值的基金；`written` = 实际写入价格的标的数
///   （价格失效信号判定依据：零变化不广播，基金无新净值不算写入）。
pub(super) fn do_incremental_sync_with<F, K, X, N>(
    conn: &Connection,
    fetch: &mut F,
    fetch_kline: &mut K,
    fetch_fx: &mut X,
    fetch_nav: &mut N,
) -> Result<SyncHoldingPricesResult>
where
    F: FnMut(&str) -> Result<Vec<StockItem>>,
    K: FnMut(&str) -> Result<Vec<KlineBar>>,
    X: FnMut(&str) -> Result<Vec<KlineBar>>,
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
{
    let held = collect_held_instruments(conn)?;
    // 单次收集全量持仓标的（一条 SQL、一个谓词引用点），Rust 内按 instrument_type
    // 分区：股票侧构造 secid 查报价与日 K；基金侧走历史净值通道（ADR-0038 决策 6）；
    // 其余（债券/ETF/其他等无行情来源）计入跳过统计——三类统计天然同源。
    let stocks: Vec<&HeldInstrument> = held.iter().filter(|i| i.is_stock()).collect();
    let funds: Vec<&HeldInstrument> = held.iter().filter(|i| i.is_fund()).collect();
    let no_quote_source = held.len() - stocks.len() - funds.len();

    // 完全无持仓：明确提示，不报错。
    if held.is_empty() {
        return Ok(SyncHoldingPricesResult {
            synced: 0,
            skipped: 0,
            message: "无持仓标的可同步".into(),
            written: 0,
        });
    }

    // 构造可查询 secid 与报价代码 → 持仓股票 映射。键为报价代码（已归一化，与响应 f12 对齐）；
    // 股票内 symbol 唯一（instruments 的 UNIQUE(symbol, instrument_type)），同代码不冲突。
    // 市场未知（unknown）无法构造 secid，计入跳过。
    let mut meta: HashMap<String, &HeldInstrument> = HashMap::new();
    let mut queryable: Vec<(String, &HeldInstrument)> = Vec::new();
    let mut skipped_unqueryable = 0usize;
    for stock in &stocks {
        if let Some(prefix) = secid_prefix(&stock.market) {
            let code = quote_code(&stock.symbol);
            meta.insert(code.to_string(), stock);
            queryable.push((format!("{prefix}.{code}"), stock));
        } else {
            skipped_unqueryable += 1;
        }
    }

    // ① 按批查询并 upsert 现价（幂等：每标的一条 market_prices 覆盖更新，原行为不变）。
    let mut synced_codes: HashSet<String> = HashSet::new();
    for chunk in queryable.chunks(ULIST_BATCH_SIZE) {
        let secids: Vec<&str> = chunk.iter().map(|(secid, _)| secid.as_str()).collect();
        let items = fetch(&secids.join(","))?;
        for item in &items {
            if let Some(stock) = meta.get(&item.code) {
                // f2≤0（停牌/无效价）经 deserialize_f2 已过滤为 None，此处跳过、保留旧价。
                if let Some(raw) = item.price {
                    let price = f2_to_price(raw, &stock.market);
                    upsert_market_price(
                        conn,
                        &stock.instrument_id,
                        price,
                        &stock.currency,
                        &crate::db::now_iso(),
                        None,
                        Some(EASTMONEY_PRICE_SOURCE),
                    )?;
                    synced_codes.insert(item.code.clone());
                }
            }
        }
    }

    // ② 近两年日 K 回填 → 周线降采样落 PriceHistory。仅覆盖股票类持仓标的
    // （口径同 InvestedInstrument）；清仓后不再采集、历史保留不删；停牌/整周无有效
    // 报价该周无点，不中断同步。
    for (secid, stock) in &queryable {
        let bars = fetch_kline(secid)?;
        for (trade_date, close) in downsample_weekly(&bars) {
            upsert_price_history(
                conn,
                &stock.instrument_id,
                &trade_date,
                price_value_to_cents(close),
                &stock.currency,
                EASTMONEY_PRICE_SOURCE,
            )?;
        }
    }

    // ③ 汇率 K 线回填 → FxRateHistory：仅持仓中的非本位币币种对（与本位币相同的
    // 无需历史折算），与价格历史同期段采集、同周规则落库。汇率消费方含基金与股票
    // 的历史市值折算，币种对取全量持仓（与分区无关）。
    let mut pairs: Vec<(String, String)> = held
        .iter()
        .map(|s| (s.currency.clone(), default_currency_code().to_string()))
        .filter(|(base, quote)| base != quote)
        .collect();
    pairs.sort();
    pairs.dedup();
    for (base, quote) in &pairs {
        let pair = format!("{base}{quote}");
        for (trade_date, rate) in downsample_weekly(&fetch_fx(&pair)?) {
            upsert_fx_rate_history(conn, base, quote, &trade_date, rate)?;
        }
    }

    // ④ 基金分区：逐只历史净值按水位增量回填（ADR-0038 决策 6），净值点降采样
    // 落周线、最新净值落现价缓存；跳过/写入统计与股票同源汇总。
    let fund_stats = sync_fund_navs(conn, &funds, fetch_nav)?;

    let synced = synced_codes.len() + fund_stats.synced;
    // 已查询但未取到有效价的（停牌/无效价/查询无果）计入跳过。
    let invalid = queryable.len() - synced_codes.len();
    let skipped = no_quote_source + skipped_unqueryable + invalid + fund_stats.skipped;
    // 实际写入 = 股票有效价 + 基金实际落库净值（基金「已是最新」不算写入）。
    let written = synced_codes.len() + fund_stats.written;

    Ok(SyncHoldingPricesResult {
        synced,
        skipped,
        written,
        message: format!("已同步 {synced} 只，跳过 {skipped} 只"),
    })
}

/// 北京时间今天（A 股/基金净值日历以北京时间为准）。
pub(super) fn beijing_today() -> NaiveDate {
    let beijing = chrono::FixedOffset::east_opt(8 * 3600).expect("固定时区偏移合法");
    chrono::Utc::now().with_timezone(&beijing).date_naive()
}

/// 近两年回填窗口起点：北京时间今天 − 2 年。A 股/港股交易日历以北京时间为准，
/// 起点精度只影响边界处至多多采一天的样本，周采样后无影响。股票日 K 与基金净值
/// 首刷窗口同此（#303，唯一实现）。
pub(super) fn two_years_ago(today: NaiveDate) -> NaiveDate {
    today
        .checked_sub_months(chrono::Months::new(24))
        .unwrap_or(today)
}

/// 近两年回填窗口起点（YYYYMMDD 形态，日 K 接口参数用）。
fn kline_beg() -> String {
    two_years_ago(beijing_today()).format("%Y%m%d").to_string()
}

/// 日线按 ISO 周降采样（ADR-0019）：每周取最后一个有报价交易日的 (日期, 收盘价)。
/// 输入日线按日期升序排序兑底（东财本就升序）；无效收盘价（≤0）与不可解析日期跳过；
/// 整周无有效报价则该周无点。周键见 [`week_monday`]。基金净值点共用本函数
///（单位净值即价格，ADR-0038 决策 3，fund_nav 攒齐全部净值点后一次降采样）。
pub(super) fn downsample_weekly(bars: &[KlineBar]) -> Vec<(String, f64)> {
    let mut sorted: Vec<&KlineBar> = bars.iter().filter(|b| b.close > 0.0).collect();
    sorted.sort_by(|a, b| a.date.cmp(&b.date));
    let mut by_week: BTreeMap<NaiveDate, (String, f64)> = BTreeMap::new();
    for bar in sorted {
        let Ok(d) = NaiveDate::parse_from_str(&bar.date, "%Y-%m-%d") else {
            continue;
        };
        // 升序遍历：后写入者即该周最后一个交易日。
        by_week.insert(week_monday(d), (bar.date.clone(), bar.close));
    }
    by_week.into_values().collect()
}

/// 该日所属 ISO 周的周一：降采样的周键，与 price_history / fx_rate_history 的
/// week_start 生成列（date(trade_date,'-6 days','weekday 1')）同口径。两侧恒等是
/// 「整周覆盖幂等」的隐式契约，由 `week_key_matches_sqlite_week_start_column` 测试绑定，
/// 防止周定义单侧调整后静默漂移。
pub(super) fn week_monday(d: NaiveDate) -> NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}

/// 生产入口：接 HTTP 层的批量报价 / 日 K / 汇率 K 线 / 历史净值页查询（复用主机池、
/// 重试、限流 pacer 与价格换算）。四个闭包串行使用，pacer 以 RefCell 共享，保证
/// 全部请求之间仍然保持统一的限速间隔。
pub(super) fn do_incremental_sync(conn: &Connection) -> Result<SyncHoldingPricesResult> {
    let client = build_client()?;
    let pacer = RefCell::new(Pacer::default());
    let beg = kline_beg();
    let mut fetch = |secids: &str| fetch_ulist(&client, &mut pacer.borrow_mut(), secids);
    let mut kline = |secid: &str| fetch_kline(&client, &mut pacer.borrow_mut(), secid, &beg);
    let mut fx = |pair: &str| fetch_fx_kline(&client, &mut pacer.borrow_mut(), pair, &beg);
    let mut nav =
        |query: &NavQuery| super::fund_nav::fetch_nav_page(&client, &mut pacer.borrow_mut(), query);
    do_incremental_sync_with(conn, &mut fetch, &mut kline, &mut fx, &mut nav)
}
