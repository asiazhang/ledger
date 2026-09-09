//! 标的信息同步编排（issue #103，issue #137 升级，issue #303 基金分区，
//! issue #695 ETF 纳入行情通道；覆盖面放开至库内全部标的 + 名称随行刷新
//! issue #827）：单次收集**库内全部标的**（不再以「当前有持仓」为界，清仓
//! 标的与纯建档未交易标的同享同步；`INVESTED_EXISTS` 谓词不再服务收集），
//! 一次执行完成五件事：① 批量报价刷股票/场内 ETF 现价 upsert `market_prices`；
//! ② 每行情分区标的一次日 K 请求回填近两年日线，本地降采样为周线落
//! `price_history`；③ 非本位币币种对的汇率 K 线同期落 `fx_rate_history`；
//! ④ 基金走历史净值通道逐只按水位增量回填（ADR-0038 决策 6，见 `fund_nav`）；
//! ⑤ 有通道的行以数据源权威名称随行刷新标的字典名称（行情通道零额外请求，
//! 基金通道逐只详情查询；「随用随修 + 同步随行刷新」，ADR-0036/0081 修订）。
//! 类型分区在 Rust 侧完成，不增删标的、不改市场。
//! 职责切分（ADR-0015，修订见 ADR-0081 / issue #827）：同步刷价格、沉淀历史、
//! 随行修名称；按代码查询/创建随用随修（全量同步翼已随 ADR-0081 决策 3
//! 退役，issue #698）。
//!
//! 编排与网络解耦：核心流程 [`do_incremental_sync_with`] 接受注入的批量报价 / 日 K /
//! 汇率 K 三个闭包（同一签名 `&str → Result<Vec<_>>`）、历史净值页闭包
//!（[`NavQuery`] → [`LsjzPage`]）、基金名称闭包（`&str → Result<String>`）与进度
//! 回调闭包（`done, total`，issue #897 / ADR-0095），测试以 mock 数据驱动（不依赖
//! 真实网络）；生产经 [`do_incremental_sync`] 接 HTTP 层（复用主机池/重试/限流
//! pacer 与价格换算）。进度回调闭包是本函数唯一的对外观察点：编排核心不碰网络、
//! 不碰事件系统，进度事件发射归壳层接线（见 `commands::sync`）。

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::model::SyncInstrumentInfoResult;
use crate::error::Result;
use crate::investment::crud::refresh_instrument_name;
use crate::investment::is_six_digit_code;
use crate::investment::prices::{
    EASTMONEY_PRICE_SOURCE, price_value_to_cents, upsert_market_price, upsert_price_history,
};
use crate::transaction::amount::default_currency_code;

use super::fund_nav::{FundSyncStats, LsjzPage, NavQuery, sync_one_fund_nav};
use super::http::{
    KlineBar, Pacer, StockItem, ULIST_BATCH_SIZE, build_client, fetch_fx_kline, fetch_kline,
    fetch_ulist, price_cents_from_raw, secid_prefix,
};
use super::persist::upsert_fx_rate_history;

/// 持仓股票的报价代码：东财 secid 与响应 f12 均为裸代码（如 600519 / 00700）。
/// 字典 symbol 可能带市场后缀（schema 注释示例格式如 "600519.SH"），取点号前段归一化。
fn quote_code(symbol: &str) -> &str {
    symbol.split('.').next().unwrap_or(symbol)
}

/// 参与同步的标的信息（一个标的一条，库内全量标的集合，单表驱动天然去重）。
pub(super) struct SyncInstrument {
    pub(super) instrument_id: String,
    pub(super) symbol: String,
    pub(super) market: String,
    pub(super) currency: String,
    pub(super) instrument_type: String,
}

impl SyncInstrument {
    /// 是否走行情通道：股票与场内 ETF（stock|etf，issue #695 / spec #690 方案 6）。
    /// 场内 ETF 与股票共用东财行情报价/日 K 接口族，按市场+代码构造 secid 同路
    /// 刷价与回填；市场未知仍无法构造 secid，照常计入跳过。
    fn is_quote_channel(&self) -> bool {
        matches!(self.instrument_type.as_str(), "stock" | "etf")
    }

    /// 是否场外基金：走历史净值通道（ADR-0038 决策 6）；名称充代码的基金行
    ///（非 6 位代码）在净值编排内计入跳过。
    fn is_fund(&self) -> bool {
        self.instrument_type == "fund"
    }
}

/// 单次收集库内全部标的（一条 SQL，无持仓前置条件，issue #827）：覆盖面从
/// 「当前有持仓」（`INVESTED_EXISTS`）放开为全库标的、按通道能力分区——清仓
/// 标的恢复同步，纯建档未交易标的首次同步按既有近两年日 K/净值回填规则补
/// 历史；无通道行（无行情类型、市场未知、名称充代码）由分区/编排自然计入
/// 跳过。`INVESTED_EXISTS` 谓词自此只服务 invested 派生列、「只看持仓」过滤
/// 与盈亏页持仓概览三处（见 `investment::predicates`）。按 symbol 升序；
/// 通道分区在 Rust 侧完成（见 [`do_incremental_sync_with`]）。
fn collect_instruments(conn: &Connection) -> Result<Vec<SyncInstrument>> {
    let sql = "SELECT i.id, i.symbol, i.market, i.currency_code, i.instrument_type \
               FROM instruments i \
               ORDER BY i.symbol";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(SyncInstrument {
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

/// 标的信息同步核心流程：单次收集库内全部标的并按通道分区 → 行情分区（stock|etf，
/// #695）构造 secid 批量报价 upsert 现价（换算按随行精度位单点）、名称随行刷新、
/// 日 K 回填周线；基金侧逐只历史净值按水位增量回填（ADR-0038 决策 6，委托
/// [`sync_one_fund_nav`]）与逐只名称刷新；汇率 K 线同期落 `fx_rate_history` →
/// 结果统计。六个回调均由调用方注入（五个抓取 + 一个进度回调，issue #897；生产接
/// HTTP 层与事件发射，测试注入 mock），本函数不触碰网络、不碰事件系统。
/// 返回统计：`synced` = 处理成功的标的数（行情分区有效价 + 基金处理成功，含基金
/// 「已是最新」）；`skipped` = 无通道行（无行情类型/市场未知/名称充代码）、停牌/
/// 无效价/查询无果与首刷查无净值的基金；`written` = 实际写入价格的标的数，
/// `renamed` = 名称被刷新的标的数（两者共同决定价格失效信号：零变化不广播，
/// 基金无新净值不算价格写入，issue #827）。
///
/// 进度回调（issue #897 / ADR-0095）：`progress(done, total)`——分母 `total` 为
/// **有通道标的数**（可构造查询的行情标的 + 有真实代码的基金；无通道行不计），
/// 收集与分区完成后立即发 `(0, total)`；此后每完成一个有通道标的推进一格
///（报价+日 K 合并为行情标的一格，净值+名称合并为基金一格；停牌/查询无果/
/// 「已是最新」照常推进——有通道标的不以成败计格）。`total` 为 0（全部无通道）
/// 不发任何进度事件，空转不伪装成推进。
pub(super) fn do_incremental_sync_with<F, K, X, N, M, P>(
    conn: &Connection,
    fetch: &mut F,
    fetch_kline: &mut K,
    fetch_fx: &mut X,
    fetch_nav: &mut N,
    fetch_fund_name: &mut M,
    progress: &mut P,
) -> Result<SyncInstrumentInfoResult>
where
    F: FnMut(&str) -> Result<Vec<StockItem>>,
    K: FnMut(&str) -> Result<Vec<KlineBar>>,
    X: FnMut(&str) -> Result<Vec<KlineBar>>,
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
    // 基金名称闭包（issue #827）：6 位代码 → 数据源权威名称；空串表示未取到
    // （不落库）。生产接基金详情通道，测试注入 mock。
    M: FnMut(&str) -> Result<String>,
    // 进度回调闭包（issue #897 / ADR-0095）：(done, total)，逐有通道标的推进；
    // 生产接事件发射（壳层接线），测试注入记录闭包。
    P: FnMut(usize, usize),
{
    let held = collect_instruments(conn)?;
    // 单次收集库内全部标的（一条 SQL，无持仓前置，issue #827），Rust 内按通道能力
    // 分区：行情分区（stock|etf，issue #695）构造 secid 查报价与日 K；基金侧走历史
    // 净值通道（ADR-0038 决策 6）；其余（债券/其他、市场未知自建行等无通道）计入
    // 跳过统计——三类统计天然同源。
    let quote_channel: Vec<&SyncInstrument> =
        held.iter().filter(|i| i.is_quote_channel()).collect();
    let funds: Vec<&SyncInstrument> = held.iter().filter(|i| i.is_fund()).collect();
    let no_quote_source = held.len() - quote_channel.len() - funds.len();

    // 库内无任何标的：明确提示，不报错。
    if held.is_empty() {
        return Ok(SyncInstrumentInfoResult {
            synced: 0,
            skipped: 0,
            message: "暂无标的可同步".into(),
            written: 0,
            renamed: 0,
        });
    }

    // 构造可查询 secid 与报价代码 → 行情分区标的 映射。键为报价代码（已归一化，
    // 与响应 f12 对齐）；行情分区内 symbol 唯一（instruments 的
    // UNIQUE(symbol, instrument_type)），同代码不冲突。
    // 市场未知（unknown）无法构造 secid，计入跳过。
    let mut meta: HashMap<String, &SyncInstrument> = HashMap::new();
    let mut queryable: Vec<(String, &SyncInstrument)> = Vec::new();
    let mut skipped_unqueryable = 0usize;
    for inst in &quote_channel {
        if let Some(prefix) = secid_prefix(&inst.market) {
            let code = quote_code(&inst.symbol);
            meta.insert(code.to_string(), inst);
            queryable.push((format!("{prefix}.{code}"), inst));
        } else {
            skipped_unqueryable += 1;
        }
    }

    // 进度分母（issue #897 / ADR-0095）：有通道标的数 = 可构造查询的行情分区
    // 标的（市场未知无法构造 secid，计入跳过、不进分母）+ 有真实代码（6 位）的
    // 基金（名称充代码行计入跳过、不进分母）。可拉取基金集合单点派生——分母、
    // 基金循环与跳过计数共用同一集合，done == total 不变量由结构保证；
    // 收集与分区完成后立即发 total；total 为 0 不发任何进度事件。
    let syncable_funds: Vec<&SyncInstrument> = funds
        .iter()
        .filter(|f| is_six_digit_code(&f.symbol))
        .copied()
        .collect();
    let skipped_name_as_code = funds.len() - syncable_funds.len();
    let total = queryable.len() + syncable_funds.len();
    if total > 0 {
        progress(0, total);
    }
    let mut done = 0usize;

    // ① 按批查询并 upsert 现价（幂等：每标的一条 market_prices 覆盖更新，原行为不变），
    // 名称随行刷新（issue #827）：批量报价响应携带数据源权威名称（f14），零额外请求，
    // 与价格解耦——停牌无价仍刷名称。报价 + 日 K 合并为该标的一格（issue #897）：
    // 批内逐只回填日 K 后推进一格，停牌/查询无果照常推进。
    let mut synced_codes: HashSet<String> = HashSet::new();
    let mut renamed = 0usize;
    for chunk in queryable.chunks(ULIST_BATCH_SIZE) {
        let secids: Vec<&str> = chunk.iter().map(|(secid, _)| secid.as_str()).collect();
        let items = fetch(&secids.join(","))?;
        for item in &items {
            if let Some(inst) = meta.get(&item.code) {
                // 名称随行刷新（issue #827）：以数据源权威名称覆盖（仅实际变化才落库）。
                if refresh_instrument_name(conn, &inst.instrument_id, &item.name)? {
                    renamed += 1;
                }
                // f2≤0（停牌/无效价）经 deserialize_positive_f64 已过滤为 None，此处跳过、保留旧价。
                if let Some(raw) = item.price {
                    // 换算按随行精度位单点（场内 ETF 三位小数报价，#695；缺 f1 按市场回退）。
                    let price = price_cents_from_raw(raw, item.precision, &inst.market);
                    upsert_market_price(
                        conn,
                        &inst.instrument_id,
                        price,
                        &inst.currency,
                        &crate::db::now_iso(),
                        None,
                        Some(EASTMONEY_PRICE_SOURCE),
                    )?;
                    synced_codes.insert(item.code.clone());
                }
            }
        }

        // ② 近两年日 K 回填 → 周线降采样落 PriceHistory（批内逐只，与报价合并为
        // 该标的一格）。覆盖行情分区全部标的（stock|etf，#695；清仓标的自 #827
        // 恢复采集）；停牌/整周无有效报价该周无点，不中断同步。
        for (secid, inst) in chunk {
            let bars = fetch_kline(secid)?;
            for (trade_date, close) in downsample_weekly(&bars) {
                upsert_price_history(
                    conn,
                    &inst.instrument_id,
                    &trade_date,
                    price_value_to_cents(close),
                    &inst.currency,
                    EASTMONEY_PRICE_SOURCE,
                )?;
            }
            done += 1;
            progress(done, total);
        }
    }

    // ③ 汇率 K 线回填 → FxRateHistory：仅非本位币币种对（与本位币相同的
    // 无需历史折算），与价格历史同期段采集、同周规则落库。汇率消费方含基金与股票
    // 的历史市值折算，币种对取全量标的（与分区无关）。
    let native = default_currency_code(conn)?;
    let mut pairs: Vec<(String, String)> = held
        .iter()
        .map(|s| (s.currency.clone(), native.clone()))
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

    // ④⑤ 基金分区逐只（issue #897 逐只合并推进）：历史净值按水位增量回填
    //（ADR-0038 决策 6，委托 [`sync_one_fund_nav`]）+ 权威名称随行刷新（issue
    // #827，净值报文不携带名称，逐只经基金详情通道；每只有码基金一请求）合并为
    // 该基金的一格——净值与名称都完成才推进；「已是最新（无新净值）」同样推进。
    // 名称充代码行（非 6 位）无通道：不进 syncable_funds（不进分母、零请求），
    // 计数由 len 差值派生（见上方 skipped_name_as_code）。
    let mut fund_stats = FundSyncStats {
        synced: 0,
        skipped: 0,
        written: 0,
    };
    for fund in &syncable_funds {
        sync_one_fund_nav(conn, fund, fetch_nav, &mut fund_stats)?;
        let name = fetch_fund_name(&fund.symbol)?;
        if refresh_instrument_name(conn, &fund.instrument_id, &name)? {
            renamed += 1;
        }
        done += 1;
        progress(done, total);
    }

    let synced = synced_codes.len() + fund_stats.synced;
    // 已查询但未取到有效价的（停牌/无效价/查询无果）计入跳过。
    let invalid = queryable.len() - synced_codes.len();
    let skipped =
        no_quote_source + skipped_unqueryable + invalid + skipped_name_as_code + fund_stats.skipped;
    // 实际写入 = 股票有效价 + 基金实际落库净值（基金「已是最新」不算写入）。
    let written = synced_codes.len() + fund_stats.written;

    Ok(SyncInstrumentInfoResult {
        synced,
        skipped,
        written,
        renamed,
        message: format!("已同步 {synced} 只，跳过 {skipped} 只"),
    })
}

/// 北京时间今天（A 股/基金净值日历以北京时间为准）。北京日历日 = UTC 时刻
/// 加 8 小时后取日期部分，UTC+8 算术直接得日期、无 Option 无 expect
///（ADR-0060 的 A 类临时豁免已由 #434 结构性消除）。
pub(super) fn beijing_today() -> NaiveDate {
    beijing_date(chrono::Utc::now())
}

/// [`beijing_today`] 的纯函数核：UTC 时刻加 8 小时后取日期部分即北京日历日
///（16:00 UTC 是北京午夜边界，边界语义由 `beijing_date_shifts_utc_by_plus_8h` 钉住）。
pub(super) fn beijing_date(now: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    (now + chrono::Duration::hours(8)).date_naive()
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

/// 生产入口：接 HTTP 层的批量报价 / 日 K / 汇率 K 线 / 历史净值页 / 基金详情查询
///（复用主机池、重试、限流 pacer 与价格换算）。五个抓取闭包串行使用，pacer 以 RefCell
/// 共享，保证全部请求之间仍然保持统一的限速间隔。进度回调透传调用方（生产接
/// 事件发射，issue #897）。
pub fn do_incremental_sync<P>(
    conn: &Connection,
    progress: &mut P,
) -> Result<SyncInstrumentInfoResult>
where
    P: FnMut(usize, usize),
{
    let client = build_client()?;
    let pacer = RefCell::new(Pacer::default());
    let beg = kline_beg();
    let mut fetch = |secids: &str| fetch_ulist(&client, &mut pacer.borrow_mut(), secids);
    let mut kline = |secid: &str| fetch_kline(&client, &mut pacer.borrow_mut(), secid, &beg);
    let mut fx = |pair: &str| fetch_fx_kline(&client, &mut pacer.borrow_mut(), pair, &beg);
    let mut nav =
        |query: &NavQuery| super::fund_nav::fetch_nav_page(&client, &mut pacer.borrow_mut(), query);
    let mut fund_name =
        |code: &str| super::fetch_fund_detail_production(code).map(|detail| detail.name);
    do_incremental_sync_with(
        conn,
        &mut fetch,
        &mut kline,
        &mut fx,
        &mut nav,
        &mut fund_name,
        progress,
    )
}
