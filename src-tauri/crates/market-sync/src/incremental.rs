//! 标的信息同步编排（issue #103，issue #137 升级，issue #303 基金分区，
//! issue #695 ETF 纳入行情通道；覆盖面放开至库内全部标的 + 名称随行刷新
//! issue #827）：单次收集**库内全部标的**（不再以「当前有持仓」为界，清仓
//! 标的与纯建档未交易标的同享同步；`INVESTED_EXISTS` 谓词不再服务收集），
//! 一次执行完成五件事：① 批量报价刷股票/场内 ETF 现价 upsert `market_prices`；
//! ② 每行情分区标的一次日 K 请求回填近两年日线，本地降采样为周线落
//! `price_history`；③ 非本位币币种对的汇率 K 线同期落 `fx_rate_history`；
//! ④ 基金走历史净值通道逐只回填（ADR-0038 决策 6，见 `fund_nav`）——无历史
//! 序列者首刷回填近两年（优先单请求全量通道、失败回退分页，issue #1062），
//! 已有序列者按水位增量（issue #1059）；
//! ⑤ 有通道的行以数据源权威名称随行刷新标的字典名称（行情通道零额外请求，
//! 基金通道逐只详情查询；「随用随修 + 同步随行刷新」，ADR-0036/0081 修订）。
//! 单只标的的历史回填整只一次提交（ADR-0122 决策 8 / issue #1373）：日 K 周线
//! 与基金净值周线的落库各自在一只一个事务内，第 N 个周点写入失败或中途中断
//! 整体回滚——不留半根历史，「有历史序列」与「历史完整」等价。
//! 类型分区在 Rust 侧完成，不增删标的、不改市场。
//! 职责切分（ADR-0015，修订见 ADR-0081 / issue #827）：同步刷价格、沉淀历史、
//! 随行修名称；按代码查询/创建随用随修（全量同步翼已随 ADR-0081 决策 3
//! 退役，issue #698）。
//!
//! 编排与网络解耦：核心流程 [`do_incremental_sync_with`] 接受注入的批量报价 / 日 K /
//! 汇率 K 三个闭包（同一签名 `&str → Result<Vec<_>>`）、历史净值页闭包
//!（[`NavQuery`] → [`LsjzPage`]）、单请求全量净值闭包（`&str → Result<Vec<NavPoint>>`，
//! 首刷深回填用，issue #1062）、基金名称闭包（`&str → Result<String>`）与进度回调
//! 闭包（`done, total`，issue #897），测试以 mock 数据驱动（不依赖真实网络）；
//! 生产经 [`super::channels`] 的通道束接 HTTP 层（复用主机池/重试/限流 pacer
//! 与价格换算）。进度回调闭包是本函数唯一的对外观察点：编排核心不碰网络、不碰事件
//! 系统，进度事件发射归壳层接线（见 `commands::sync`）。
//!
//! 取数面（ADR-0121 / issue #1374）：基金名称与场外基金现价改走**批量取数面**
//!（[`super::bulk`]：名称全量字典 + 场外基金净值全市场批量面，各整次同步最多一次
//! 请求），批量面失败 / 停用 / 未覆盖（缺口）一律 fail-closed 回退既有逐标的通道；
//! 缺口与失败在日志与统计上分开，缺口不触发熔断。
//!
//! 编排与连接解耦（issue #1275 作用域会话接缝）：本模块所有函数的签名里没有
//! 连接句柄——读写库一律经注入的 [`ScopedSession`] 短暂取一次连接，网络抓取
//! 只发生在会话之外。「持着连接做网络 I/O」在类型上不可表达；会话实现在壳层
//! 接线（生产 = 分段写入口的短暂取锁会话，见 `commands::sync`）。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::bulk::{BulkCoverage, BulkFetchSurfaces, FundNameDictionary, FundNavTable};
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::crud::refresh_instrument_name;
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, price_value_to_cents, upsert_market_price,
    upsert_price_history,
};
use ledger_investment::{InstrumentType, PriceChannel, derive_price_channel};
use ledger_transaction::amount::default_currency_code;

use super::fund_nav::{FundSyncStats, LsjzPage, NavPoint, NavQuery, sync_one_fund_nav};
use super::http::{KlineBar, StockItem, ULIST_BATCH_SIZE, price_cents_from_raw, secid_prefix};
use super::persist::upsert_fx_rate_history;
use super::progress::{FundNavProgress, SyncProgress};
use super::session::ScopedSession;

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
    /// 价格写入通道（issue #1060）：投资域派生单点 [`derive_price_channel`] 的
    /// 判定结果——行情（Quote）/ 净值（FundNav）两分区参与同步，手动报价与
    /// 无来源行计入跳过。分区口径与标的读投影（`Instrument::price_channel`）
    /// 同源单点，不再各自镜像类型与市场判定。
    pub(super) channel: PriceChannel,
}

/// 单次收集库内全部标的（一条 SQL，无持仓前置条件，issue #827）：覆盖面从
/// 「当前有持仓」（`INVESTED_EXISTS`）放开为全库标的、按通道能力分区——清仓
/// 标的恢复同步，纯建档未交易标的首次同步按既有近两年日 K/净值回填规则补
/// 历史；无通道行（无行情类型、市场未知、名称充代码）由分区/编排自然计入
/// 跳过。`INVESTED_EXISTS` 谓词自此只服务标的列表持仓派生标记、「只看持仓」
/// 过滤、持仓页签概览与价格过期提示的持仓缺现价判定（完整清单见
/// `investment::predicates`，不在此复述处数）。按 symbol 升序；
/// 通道分区在 Rust 侧完成（见 [`do_incremental_sync_with`]）。
fn collect_instruments(conn: &Connection) -> Result<Vec<SyncInstrument>> {
    let sql = "SELECT i.id, i.symbol, i.market, i.currency_code, i.instrument_type \
               FROM instruments i \
               ORDER BY i.symbol";
    let mut stmt = conn.prepare(sql)?;
    // 价格通道收集时单点派生（issue #1060）：分区判定不再留在同步域镜像
    // （原 is_quote_channel / is_fund + 6 位代码过滤），与标的读投影同源。
    let rows = stmt.query_map([], |r| {
        let symbol: String = r.get(1)?;
        let market: String = r.get(2)?;
        let kind: InstrumentType = r.get(4)?;
        Ok(SyncInstrument {
            instrument_id: r.get(0)?,
            channel: derive_price_channel(kind, &market, &symbol),
            symbol,
            market,
            currency: r.get(3)?,
        })
    })?;
    let mut instruments = Vec::new();
    for row in rows {
        instruments.push(row?);
    }
    Ok(instruments)
}

/// 本次同步从批量取数面取回的数据（ADR-0121 / issue #1374）。
struct BulkData {
    /// 名称全量字典（None = 未命中 / 未尝试 → 逐只名称通道兜底）。
    names: Option<FundNameDictionary>,
    /// 场外基金净值批量面（None = 未命中 / 未尝试 → 逐只净值通道兜底）。
    nav: Option<FundNavTable>,
    /// 本次同步是否降级（批量面失败或处于停用期）：降级对用户可见的事实位
    ///（文案接线归 issue #1376，本票先落事实与日志统计）。
    degraded: bool,
}

impl BulkData {
    /// 无标的可刷（净值分区为空）：不尝试也不降级——零请求是「没得刷」的自然结果。
    fn none() -> Self {
        Self {
            names: None,
            nav: None,
            degraded: false,
        }
    }

    /// 本次同步没走上批量取数面（停用期或面失败）：降级，逐标的通道兜底。
    fn unavailable() -> Self {
        Self {
            names: None,
            nav: None,
            degraded: true,
        }
    }
}

/// 取两个批量取数面（ADR-0121 决策 1/3）：先场外基金净值批量面、后名称全量字典
/// ——价格是本动作的主产出。两个面各整次同步**最多一次请求**。
///
/// 熔断（决策 3）：跨同步记忆判定处于停用期时一面都不试（零请求，直接回退逐标的
/// 通道）；任一面失败即本次同步熔断——后续**不再尝试任何批量面**（本函数是批量面
/// 的唯一调用点，故按条缺口补齐的逐条回退不会再撞一次批量面）。逐标的通道随后
/// fail-closed 兜底，价格与名称照常落库。
///
/// 失败与缺口是两件事：失败进跨同步记忆、降级事实与 warn 日志；缺口（批量面没
/// 收录该标的）只体现在逐条回退的 debug 日志与 `bulk_gaps` 统计上，不触发熔断。
fn fetch_bulk_surfaces(bulk: &mut BulkFetchSurfaces) -> BulkData {
    let now = Instant::now();
    // 状态判定在锁内、网络请求在锁外（ADR-0069 决策 4 同款纪律：网络等待不进锁）。
    let allowed = match bulk.circuit.lock() {
        Ok(mut circuit) => circuit.should_attempt(now),
        Err(error) => {
            tracing::warn!(%error, "批量取数面跨同步记忆互斥体损坏，本次同步按降级处理");
            false
        }
    };
    if !allowed {
        tracing::info!("批量取数面处于停用期（连续失败达阈值），本次同步全部走逐标的通道");
        return BulkData::unavailable();
    }

    let nav = take_bulk_surface("场外基金净值批量面", &mut bulk.nav);
    // 同步内熔断（决策 3）：一面失败即本次同步不再尝试其余批量面——否则每只标的
    // 都先试一次批量面再回退，请求量比改造前更多。
    let names = if nav.is_some() {
        take_bulk_surface("基金名称全量字典", &mut bulk.names)
    } else {
        None
    };
    let failed = nav.is_none() || names.is_none();

    match bulk.circuit.lock() {
        Ok(mut circuit) => {
            if failed {
                circuit.record_failure(now);
            } else {
                circuit.record_success();
            }
        }
        Err(error) => tracing::warn!(%error, "批量取数面跨同步记忆互斥体损坏，本次结果未记入"),
    }

    BulkData {
        names,
        nav,
        degraded: failed,
    }
}

/// 取一个批量面：命中记覆盖规模（debug），失败记 warn 并返回 None——由调用方按
/// 熔断契约处置（一面失败即本次同步不再尝试其余面，见 [`fetch_bulk_surfaces`]）。
fn take_bulk_surface<T: BulkCoverage>(
    surface: &'static str,
    fetch: &mut impl FnMut() -> Result<T>,
) -> Option<T> {
    match fetch() {
        Ok(data) => {
            tracing::debug!(surface, covered = data.covered(), "行情批量取数面命中");
            Some(data)
        }
        Err(error) => {
            tracing::warn!(surface, %error, "行情批量取数面失败，本次同步回退逐标的通道");
            None
        }
    }
}

/// 标的信息同步核心流程：单次收集库内全部标的并按通道分区 → 行情分区（stock|etf，
/// #695）构造 secid 批量报价 upsert 现价（换算按随行精度位单点）、名称随行刷新、
/// 日 K 回填周线；基金侧先取两个批量取数面（ADR-0121，未命中 / 失败 / 停用一律
/// 回退逐标的通道），再逐只历史净值按水位增量回填（ADR-0038 决策 6，委托
/// [`sync_one_fund_nav`]）与逐只名称刷新；汇率 K 线同期落 `fx_rate_history` →
/// 结果统计。注入面 = 六个抓取闭包 + 批量取数面 + 一个进度回调（issue #897；生产接
/// HTTP 层与事件发射，测试注入 mock），本函数不触碰网络、不碰事件系统。
/// 返回统计：`synced` = 处理成功的标的数（行情分区有效价 + 基金处理成功，含基金
/// 「已是最新」）；`skipped` = 无通道行（无行情类型/市场未知/名称充代码）、停牌/
/// 无效价/查询无果、首刷查无净值与空响应（疑似被拦截/异常，issue #1059）的基金；
/// `written` = 实际写入价格的标的数，
/// `renamed` = 名称被刷新的标的数（两者共同决定价格失效信号：零变化不广播，
/// 基金无新净值不算价格写入，issue #827）。
///
/// 进度回调（issue #897 / ADR-0095；页级明细 issue #1061）：载荷 `done`/`total`
/// 的分母 `total` 为**有通道标的数**（可构造查询的行情标的 + 有真实代码的基金；
/// 无通道行不计），收集与分区完成后立即发 `{ done: 0, total }`；此后每完成一个
/// 有通道标的推进一格（报价+日 K 合并为行情标的一格，净值+名称合并为基金一格；
/// 停牌/查询无果/「已是最新」照常推进——有通道标的不以成败计格）。`total` 为 0
///（全部无通道）不发任何进度事件，空转不伪装成推进。首刷/深回填的基金在页抓取
/// 返回后额外带出 `fund` 页级明细（不改 `done`/`total`；单页不发）。
///
/// 写入见证（issue #1277）：每个实际写入点（行情报价落库 / 名称随行刷新 /
/// 基金净值落库 / 基金名称刷新）在落库成功后标记 [`WriteWitness`]——中途失败的
/// 运行结果统计随错误丢失，见证器由调用方持有（`&mut` 传入）存活，壳层据此把
/// 「实际写过」的失败收尾归一为证据（成败同判，见 `commands::sync`）。
/// 取数面注入（ADR-0121 / issue #1374）：`bulk` 是名称全量字典 + 场外基金净值
/// 全市场批量面 + 跨同步记忆的打包束（生产接 HTTP 层、测试注入桩，见
/// [`super::channels`]）。批量面只回答「这次刷新用几次请求」，不改变价格来源归属。
// 六个逐标的抓取闭包 + 取数面 + 会话 + 进度回调 + 写入见证共 10 参：网络接缝
// 逐通道注入使然（与 HTTP 层 request_from_hosts 同形），参数表就是「本编排消费
// 哪些外部通道」的清单。
#[allow(clippy::too_many_arguments)]
pub(super) fn do_incremental_sync_with<Q, F, K, X, N, S, M, P>(
    session: &Q,
    fetch: &mut F,
    fetch_kline: &mut K,
    fetch_fx: &mut X,
    fetch_nav: &mut N,
    fetch_nav_full: &mut S,
    fetch_fund_name: &mut M,
    bulk: &mut BulkFetchSurfaces,
    progress: &mut P,
    witness: &mut WriteWitness,
) -> Result<SyncInstrumentInfoResult>
where
    // 作用域会话接缝（issue #1275）：读写库的唯一通道，签名层面取不到连接。
    Q: ScopedSession,
    F: FnMut(&str) -> Result<Vec<StockItem>>,
    K: FnMut(&str) -> Result<Vec<KlineBar>>,
    X: FnMut(&str) -> Result<Vec<KlineBar>>,
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
    // 单请求全量净值闭包（issue #1062）：6 位基金代码 → 整只基金历史单位净值；
    // 仅首刷深回填用，失败由 sync_one_fund_nav fail-closed 回退 fetch_nav 分页通道。
    S: FnMut(&str) -> Result<Vec<NavPoint>>,
    // 基金名称闭包（issue #827）：6 位代码 → 数据源权威名称；空串表示未取到
    // （不落库）。生产接基金详情通道，测试注入 mock。
    M: FnMut(&str) -> Result<String>,
    // 进度回调闭包（issue #897 / ADR-0095；页级明细 issue #1061）：三字段载荷，
    // 逐有通道标的推进、基金深回填带页级明细；生产接事件发射（壳层接线），
    // 测试注入记录闭包。
    P: FnMut(SyncProgress),
{
    let held = session.with_connection(collect_instruments)?;
    // 单次收集库内全部标的（一条 SQL，无持仓前置，issue #827），按投资域派生的
    // 价格通道分区（issue #1060，判定单点 `derive_price_channel`）：行情分区
    // 构造 secid 查报价与日 K；净值分区（fund 且 6 位真实代码，ADR-0038 决策 6）
    // 走历史净值通道；其余（手动报价通道与无来源行：债券/其他、市场未知自建行、
    // 名称充代码基金行等）计入跳过统计——三类统计天然同源。
    let quote_channel: Vec<&SyncInstrument> = held
        .iter()
        .filter(|i| i.channel == PriceChannel::Quote)
        .collect();
    let funds: Vec<&SyncInstrument> = held
        .iter()
        .filter(|i| i.channel == PriceChannel::FundNav)
        .collect();
    let no_quote_source = held.len() - quote_channel.len() - funds.len();

    // 库内无任何标的：明确提示，不报错。
    if held.is_empty() {
        return Ok(SyncInstrumentInfoResult {
            synced: 0,
            skipped: 0,
            message: "暂无标的可同步".into(),
            written: 0,
            renamed: 0,
            bulk_degraded: false,
            bulk_gaps: 0,
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

    // 进度分母（issue #897 / ADR-0095）：有通道标的数 = 行情分区标的 + 净值分区
    // 基金（价格通道派生单点已保证行情分区市场可查、净值分区代码为 6 位真实代码；
    // 无通道行不进分母）。收集与分区完成后立即发 total；total 为 0 不发任何进度事件。
    let total = queryable.len() + funds.len();
    if total > 0 {
        progress(SyncProgress::instrument(0, total));
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
        // 批量报价是网络请求，在会话之外；响应落库（名称随行刷新 + 现价 upsert）
        // 才短暂取一次连接（issue #1275）。
        let items = fetch(&secids.join(","))?;
        for item in &items {
            if let Some(inst) = meta.get(&item.code) {
                session.with_connection(|conn| {
                    // 名称随行刷新（issue #827）：以数据源权威名称覆盖（仅实际变化才落库）。
                    if refresh_instrument_name(conn, &inst.instrument_id, &item.name)? {
                        renamed += 1;
                        witness.mark_written();
                    }
                    // f2≤0（停牌/无效价）经 deserialize_positive_f64 已过滤为 None，此处跳过、保留旧价。
                    if let Some(raw) = item.price {
                        // 换算按随行精度位单点（场内 ETF 三位小数报价，#695；缺 f1 按市场回退）。
                        let price = price_cents_from_raw(raw, item.precision, &inst.market);
                        upsert_market_price(
                            conn,
                            &MarketPriceWrite {
                                instrument_id: &inst.instrument_id,
                                price_cents: price,
                                currency_code: &inst.currency,
                                // 场内现价时点 = 写入时刻、无净值日期语义（ADR-0036）。
                                priced_at: &ledger_infra::db::now_iso(),
                                nav_date: None,
                                source: Some(EASTMONEY_PRICE_SOURCE),
                            },
                        )?;
                        synced_codes.insert(item.code.clone());
                        witness.mark_written();
                    }
                    Ok(())
                })?;
            }
        }

        // ② 近两年日 K 回填 → 周线降采样落 PriceHistory（批内逐只，与报价合并为
        // 该标的一格）。覆盖行情分区全部标的（stock|etf，#695；清仓标的自 #827
        // 恢复采集）；停牌/整周无有效报价该周无点，不中断同步。单只整只一次提交
        //（ADR-0122 决策 8 / issue #1373）：第 N 个周点写入失败整体回滚，不留半根
        // 历史——「有历史序列」与「历史完整」由此等价。事务经 [`ensure_transaction`]
        //（ADR-0033 嵌套感知）：autocommit 连接自持事务、已在事务中则加入外层。
        for (secid, inst) in chunk {
            // 日 K 抓取在会话之外；降采样落库才短暂取一次连接（issue #1275）。
            let bars = fetch_kline(secid)?;
            session.with_connection(|conn| {
                ensure_transaction(conn, || {
                    write_weekly_price_history(conn, &inst.instrument_id, &inst.currency, &bars)
                })
            })?;
            done += 1;
            progress(SyncProgress::instrument(done, total));
        }
    }

    // ③ 汇率 K 线回填 → FxRateHistory：仅非本位币币种对（与本位币相同的
    // 无需历史折算），与价格历史同期段采集、同周规则落库。汇率消费方含基金与股票
    // 的历史市值折算，币种对取全量标的（与分区无关）。汇率落库不计入写入见证
    //（issue #1277）：与成功路径的零写入判定同口径——只有价格或名称写入才发
    // 价格失效信号，汇率历史变化不在其列。
    let native = session.with_connection(default_currency_code)?;
    let mut pairs: Vec<(String, String)> = held
        .iter()
        .map(|s| (s.currency.clone(), native.clone()))
        .filter(|(base, quote)| base != quote)
        .collect();
    pairs.sort();
    pairs.dedup();
    for (base, quote) in &pairs {
        let pair = format!("{base}{quote}");
        // 汇率 K 线抓取在会话之外；降采样落库才短暂取一次连接（issue #1275）。
        let bars = fetch_fx(&pair)?;
        session.with_connection(|conn| {
            for (trade_date, rate) in downsample_weekly(&bars) {
                upsert_fx_rate_history(conn, base, quote, &trade_date, rate)?;
            }
            Ok(())
        })?;
    }

    // ④ 批量取数面（ADR-0121 / issue #1374）：名称全量字典 + 场外基金净值全市场
    // 批量面，各整次同步最多一次请求——请求量自此不再随基金数线性增长；净值分区
    // 为空则零请求（无标的可刷，不白撞数据源）。失败 / 停用 / 未覆盖的标的在下方
    // 逐标的通道 fail-closed 兜底（缺口与失败分开统计，见 [`fetch_bulk_surfaces`]）。
    let mut bulk_gaps = 0usize;
    let bulk_data = if funds.is_empty() {
        BulkData::none()
    } else {
        fetch_bulk_surfaces(bulk)
    };

    // ⑤ 基金分区逐只（issue #897 逐只合并推进）：历史净值回填（ADR-0038 决策 6，
    // 委托 [`sync_one_fund_nav`]——无历史序列者首刷近两年、已有序列者按净值日期
    // 水位增量，issue #1059）+ 权威名称随行刷新（issue #827）合并为该基金的一格；
    // 「已是最新（无新净值）」同样推进。名称与耗时随取数面改写：批量面命中即零
    // 请求（名称全量字典 / 净值批量面的最新净值日期即「是否有新净值」的判据），
    // 未覆盖的标的退回既有逐标的通道（名称走基金详情通道、净值走 lsjz 分页）。
    // 名称充代码行无通道：不进净值分区（不进分母、零请求），计入跳过（见上）。
    let mut fund_stats = FundSyncStats {
        synced: 0,
        skipped: 0,
        written: 0,
    };
    for fund in &funds {
        // 页级推进（issue #1061）：`done`/`total` 仍是标的级口径，页抓取返回后
        // 才带出本基金的页明细——抓取内部的退避/重试等待不产生推进。
        let code = fund.symbol.clone();
        // 净值批量面命中 → 逐只同步以「最新净值日期」作水位判据（无新净值即整只
        // 零请求）；未覆盖 = 缺口，逐条回退逐标的通道补齐，不触发熔断。
        let latest_hint = bulk_data.nav.as_ref().and_then(|table| table.get(&code));
        if bulk_data.nav.is_some() && latest_hint.is_none() {
            bulk_gaps += 1;
            tracing::debug!(
                code = %code,
                "场外基金净值批量面未覆盖该标的（新成立 / 已终止 / 清盘 / 部分货币基金），逐只通道补齐"
            );
        }
        {
            let mut on_page = |page: u64, pages: u64| {
                progress(SyncProgress {
                    done,
                    total,
                    fund: Some(FundNavProgress {
                        code: code.clone(),
                        page,
                        pages,
                    }),
                });
            };
            let written_before = fund_stats.written;
            sync_one_fund_nav(
                session,
                fund,
                latest_hint,
                fetch_nav,
                fetch_nav_full,
                &mut fund_stats,
                &mut on_page,
            )?;
            // 净值实际落库才标记（「已是最新」不算写入，与 fund_stats.written 同判）。
            if fund_stats.written > written_before {
                witness.mark_written();
            }
        }
        // 名称随行刷新（issue #827；取数面随 ADR-0121 改写）：全量字典命中即零
        // 请求（与净值批量面同一次同步的常数级请求量的一部分）；未覆盖的标的退回
        // 逐只详情通道——遇「确定性查无」降级为保留原名（ADR-0039 修订，
        // issue #1212）：基金可能已终止（搜索索引与档案通道都不再可达），但这不该
        // 打断整次同步；网络类失败仍按既有契约上抛。空名称 = 未取到，不落库。
        let name = match bulk_data.names.as_ref().and_then(|dict| dict.get(&code)) {
            Some(name) => name.clone(),
            None => {
                if bulk_data.names.is_some() {
                    tracing::debug!(code = %code, "名称全量字典未覆盖该标的，逐只名称通道补齐");
                }
                match fetch_fund_name(&fund.symbol) {
                    Ok(name) => name,
                    Err(error) if error.is_code("sync.fund-not-found") => {
                        tracing::warn!(
                            code = %fund.symbol,
                            "基金名称刷新查无此码（搜索索引与档案通道皆未命中），保留原名称"
                        );
                        String::new()
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        // 名称落库才短暂取一次连接（抓取在会话之外，issue #1275）。
        if session
            .with_connection(|conn| refresh_instrument_name(conn, &fund.instrument_id, &name))?
        {
            renamed += 1;
            witness.mark_written();
        }
        done += 1;
        progress(SyncProgress::instrument(done, total));
    }

    let synced = synced_codes.len() + fund_stats.synced;
    // 已查询但未取到有效价的（停牌/无效价/查询无果）计入跳过。
    let invalid = queryable.len() - synced_codes.len();
    // 无通道行（手动报价通道与无来源：债券/其他、市场未知自建行、名称充代码基金行）
    // 与停牌/查询无果/首刷查无净值等一并计入跳过。
    let skipped = no_quote_source + skipped_unqueryable + invalid + fund_stats.skipped;
    // 实际写入 = 股票有效价 + 基金实际落库净值（基金「已是最新」不算写入）。
    let written = synced_codes.len() + fund_stats.written;
    // 取数面统计收尾（ADR-0121 决策 3）：缺口与失败在统计上分开——缺口（批量面
    // 没收录该标的）逐条回退补齐、不触发熔断；失败（面报错或处于停用期）按熔断
    // 契约整体回退逐标的通道，并带出降级事实。
    if bulk_data.degraded || bulk_gaps > 0 {
        tracing::info!(
            bulk_degraded = bulk_data.degraded,
            bulk_gaps,
            "行情批量取数面统计：缺口逐条回退，失败整体降级"
        );
    }

    Ok(SyncInstrumentInfoResult {
        synced,
        skipped,
        written,
        renamed,
        bulk_degraded: bulk_data.degraded,
        bulk_gaps,
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

/// 近两年回填窗口起点（YYYYMMDD 形态，日 K 接口参数用）。生产通道束构造时
/// 取一次（`channels::SyncFetchChannels::production`，每次同步一次的口径不变）。
pub(crate) fn kline_beg() -> String {
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

/// 单只标的的周采样历史落库（ADR-0122 决策 8 / issue #1373）：日 K 回填与基金
/// 净值回填两条通道共用的「降采样 + 逐周 upsert」形体，不另写第二份采样落库。
/// 「整周覆盖」幂等由 `upsert_price_history` 的 UNIQUE 约束保证（同周重复获取
/// 零重复行）。
///
/// 本函数只写行、**不开事务**：调用方必须在**一只一个事务**里包住它
///（[`ensure_transaction`]），否则第 N 个周点写入失败会留下半根历史。两个现役
/// 调用点（行情分区日 K 回填、基金净值回填）都已如此接线；基金侧另有现价与
/// 历史同事务的需求，故事务边界留在调用方而非本函数。
pub(super) fn write_weekly_price_history(
    conn: &Connection,
    instrument_id: &str,
    currency: &str,
    bars: &[KlineBar],
) -> Result<()> {
    for (trade_date, close) in downsample_weekly(bars) {
        upsert_price_history(
            conn,
            instrument_id,
            &trade_date,
            price_value_to_cents(close),
            currency,
            EASTMONEY_PRICE_SOURCE,
        )?;
    }
    Ok(())
}

/// 该日所属 ISO 周的周一：降采样的周键，与 price_history / fx_rate_history 的
/// week_start 生成列（date(trade_date,'-6 days','weekday 1')）同口径。两侧恒等是
/// 「整周覆盖幂等」的隐式契约，由 `week_key_matches_sqlite_week_start_column` 测试绑定，
/// 防止周定义单侧调整后静默漂移。
pub(super) fn week_monday(d: NaiveDate) -> NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}
