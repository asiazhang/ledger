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
//! 汇率 K 三个闭包（日 K / 汇率 K 同签名 `&str → Result<Vec<_>>`；批量报价收
//! [「市场 + 代码」查询单元](QuoteQuery)，issue #1555）、历史净值页闭包
//!（[`NavQuery`] → [`NavPage`]）、基金名称闭包（`&str → Result<String>`）与进度回调
//! 闭包（`done, total`，issue #897），测试以 mock 数据驱动（不依赖真实网络）；
//! 生产经 [`super::channels`] 的通道束接 HTTP 层（复用主机池/重试/限流 pacer
//! 与价格换算）。进度回调闭包是本函数唯一的对外观察点：编排核心不碰网络、不碰事件
//! 系统，进度事件发射归壳层接线（见 `commands::sync`）。
//!
//! 取数面（ADR-0121 / issue #1374 / ADR-0130 决策 2）：基金名称与场外基金现价
//! 改走**批量取数面**（[`super::bulk`]：新浪 `f_` 面按代码一次请求取回名称与最新
//! 净值，整次同步最多一次逻辑请求），批量面失败 / 停用 / 未收录（缺口）一律
//! fail-closed 回退既有逐标的通道；缺口与失败在日志与统计上分开，缺口不触发熔断。
//!
//! 编排与连接解耦（issue #1275 作用域会话接缝；async 形态见 #1412 / ADR-0125
//! 决策 5）：本模块所有函数的签名里没有连接句柄——读写库一律经注入的
//! [`ScopedSession`] 短暂取一次连接（取连接作业 async、闭包内同步 rusqlite），
//! 网络抓取只发生在会话之外且以 `await` 表达。「持着连接做网络 I/O」在类型上
//! 不可表达；会话生产实现 = 门面写槽裸作业会话（[`super::session::FacadeWriteSession`]），
//! 命令壳侧与域内后台车道各自持门面句柄接线。

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::bulk::{BulkFetchSurfaces, BulkNavPoint, FetchFundBatch, FundBatch};
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use ledger_infra::error::Result;
use ledger_investment::crud::refresh_instrument_name;
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, TENCENT_PRICE_SOURCE, price_value_to_cents,
    upsert_market_price, upsert_price_history,
};
use ledger_investment::{InstrumentType, PriceChannel, derive_price_channel};
use ledger_transaction::amount::default_currency_code;

use super::channels::{FetchFuture, QuoteItem, QuoteQuery};
use super::fund_nav::{NavPage, NavQuery};
use super::fund_price_refresh::{FundSyncStats, refresh_one_fund_price};
use super::http::KlineBar;
use super::persist::upsert_fx_rate_history;
use super::progress::{FundNavProgress, SyncProgress};
use super::session::ScopedSession;

/// 持仓股票的报价代码：数据源响应 f12 与查询单元的代码均为裸代码（如 600519 / 00700）。
/// 字典 symbol 可能带市场后缀（schema 注释示例格式如 "600519.SH"），取点号前段归一化。
pub(super) fn quote_code(symbol: &str) -> &str {
    symbol.split('.').next().unwrap_or(symbol)
}

/// 参与同步的标的信息（一个标的一条，库内全量标的集合，单表驱动天然去重）。
pub(super) struct SyncInstrument {
    pub(super) instrument_id: String,
    pub(super) symbol: String,
    pub(super) market: String,
    pub(super) currency: String,
    /// 价格写入通道（issue #1060）：投资域派生单点 [`derive_price_channel`] 的
    /// 判定结果——行情（Quote）/ 净值（FundNav）两分区参与同步，恒定价格
    ///（ADR-0126 决策 4：采集链路全豁免）与手动报价、无来源行计入跳过。分区
    /// 口径与标的读投影（`Instrument::price_channel`）同源单点，不再各自镜像
    /// 类型与市场判定。
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
    let sql = "SELECT i.id, i.symbol, i.market, i.currency_code, i.instrument_type, i.constant_unit_price \
               FROM instruments i \
               ORDER BY i.symbol";
    let mut stmt = conn.prepare(sql)?;
    // 价格通道收集时单点派生（issue #1060；恒定单位价格判定输入见 ADR-0126）：
    // 分区判定不再留在同步域镜像（原 is_quote_channel / is_fund + 6 位代码
    // 过滤），与标的读投影同源。
    let rows = stmt.query_map([], |r| {
        let symbol: String = r.get(1)?;
        let market: String = r.get(2)?;
        let kind: InstrumentType = r.get(4)?;
        let constant_unit_price: Option<i64> = r.get(5)?;
        Ok(SyncInstrument {
            instrument_id: r.get(0)?,
            channel: derive_price_channel(kind, &market, &symbol, constant_unit_price),
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

/// 本次同步从批量取数面取回的数据（ADR-0121 / issue #1374 / ADR-0130 决策 2）。
struct BulkData {
    /// 取数面载荷（None = 未命中 / 未尝试 → 逐只通道兜底）：名称字典是覆盖面
    /// 判据（有名称即被面收录），净值表是「面是否给出可采信价格点」的判据
    /// ——货基错位行只在名称字典（issue #1565 / ADR-0130 决策 6）。
    batch: Option<FundBatch>,
    /// 本次同步是否降级（批量面失败或处于停用期）：降级对用户可见的事实位
    ///（文案接线归 issue #1376，本票先落事实与日志统计）。
    degraded: bool,
}

impl BulkData {
    /// 无标的可刷（净值分区为空）：不尝试也不降级——零请求是「没得刷」的自然结果。
    fn none() -> Self {
        Self {
            batch: None,
            degraded: false,
        }
    }

    /// 本次同步没走上批量取数面（停用期或面失败）：降级，逐标的通道兜底。
    fn unavailable() -> Self {
        Self {
            batch: None,
            degraded: true,
        }
    }

    /// 该码是否被本次批量面收录（有名称即收录，与是否给出价格点无关）。
    fn covers(&self, code: &str) -> bool {
        self.batch.as_ref().is_some_and(|batch| batch.covers(code))
    }

    /// 面给出的最新单位净值点（未收录或货基错位行为 None）。
    fn nav_of(&self, code: &str) -> Option<&BulkNavPoint> {
        self.batch.as_ref().and_then(|batch| batch.nav_of(code))
    }

    /// 面给出的数据源权威名称（未收录为 None）。
    fn name_of(&self, code: &str) -> Option<&str> {
        self.batch.as_ref().and_then(|batch| batch.name_of(code))
    }

    /// 本次是否尝试并命中了批量面（未尝试 / 失败为 false）：缺口日志与逐只名称
    /// 兜底日志的分流判据。
    fn has_batch(&self) -> bool {
        self.batch.is_some()
    }
}

/// 取批量取数面（ADR-0121 决策 1/3）：新浪 `f_` 场外基金批量面按本次同步现场的
/// 基金代码查询，一次请求同时取回名称与最新净值（ADR-0130 决策 2 / issue
/// #1565）。净值分区为空则零请求（无标的可刷，不白撞数据源，调用方保证）。
///
/// 熔断（决策 3）：跨同步记忆判定处于停用期时零请求、直接回退逐标的通道；本面
/// 失败即本次同步熔断——逐标的通道随后 fail-closed 兜底，价格与名称照常落库。
///
/// 失败与缺口是两件事：失败进跨同步记忆、降级事实与 warn 日志；缺口（面未收录
/// 该标的，即名称字典无该码）只体现在逐条回退的 debug 日志与 `bulk_gaps` 统计上，
/// 不触发熔断。
///
/// **货基错位行不是缺口**：`f_` 面含货基、以名称行在场但不产出价格点（万份收益
/// 在单位净值位，ADR-0130 决策 6）——它计入名称覆盖面、不计入缺口，在编排侧按
/// 「有名称、无净值点」落逐只臂经官方披露判定门收尾。
async fn fetch_bulk_surfaces(bulk: &mut BulkFetchSurfaces, codes: &[String]) -> BulkData {
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

    let batch = take_bulk_surface(&mut bulk.funds, codes).await;
    let failed = batch.is_none();

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

    match batch {
        Some(batch) => BulkData {
            batch: Some(batch),
            degraded: false,
        },
        None => BulkData::unavailable(),
    }
}

/// 取批量面：命中记覆盖面规模（debug，名称字典行数——有名称即被面收录）；失败记
/// warn 并返回 None，由调用方按熔断契约处置（失败即本次同步整体回退逐标的通道）。
async fn take_bulk_surface(fetch: &mut FetchFundBatch, codes: &[String]) -> Option<FundBatch> {
    match fetch(codes).await {
        Ok(batch) => {
            tracing::debug!(
                covered = batch.names.len(),
                nav_points = batch.nav.len(),
                "场外基金批量面命中"
            );
            Some(batch)
        }
        Err(error) => {
            tracing::warn!(%error, "场外基金批量面失败，本次同步回退逐标的通道");
            None
        }
    }
}

/// 标的信息同步核心流程（ADR-0122 / issue #1377 起只刷现价）：单次收集库内全部
/// 标的并按通道分区 → 行情分区（stock|etf，#695）递「市场 + 代码」查询单元批量报价
///（查询键由通道内部构造，issue #1555）upsert 现价
///（换算按随行精度位单点）、名称随行刷新、有历史序列者由现价刷新直落当周采样点
///（不另发逐只请求）；汇率 K 线同期落 `fx_rate_history` → 基金侧先取批量取
/// 数面（ADR-0121，未命中 / 失败 / 停用一律回退逐标的短窗），逐只刷新现价与名称
///（委托 [`refresh_one_fund_price`]）→ 结果统计。**不再回填价格历史**：首刷深
/// 回填与缺周点补齐归价格历史后台补全（[`super::history`]）。
/// 注入面 = 四个抓取闭包 + 批量取数面 + 一个进度回调（issue #897；生产接
/// HTTP 层与事件发射，测试注入 mock），本函数不触碰网络、不碰事件系统。
/// 返回统计：`synced` = 处理成功的标的数（行情分区有效价 + 基金处理成功，含基金
/// 「已是最新」）；`skipped` = 无通道行（无行情类型/市场未知/名称充代码）、停牌/
/// 无效价/查询无果、查无净值与空响应（疑似被拦截/异常，issue #1059）的基金；
/// `written` = 实际写入价格的标的数，
/// `renamed` = 名称被刷新的标的数（两者共同决定价格失效信号：零变化不广播，
/// 基金无新净值不算价格写入，issue #827）。
///
/// 进度回调（issue #897 / ADR-0095；页级明细 issue #1061）：载荷 `done`/`total`
/// 的分母 `total` 为**有通道标的数**（可构造查询的行情标的 + 有真实代码的基金；
/// 恒定价格行与无通道行不计，ADR-0126 决策 4），收集与分区完成后立即发
/// `{ done: 0, total }`；此后每完成一个有通道标的推进一格（现价+名称合并为各通道标的的一格；
/// 停牌/查询无果/「已是最新」照常推进——有通道标的不以成败计格）。`total` 为 0
///（全部无通道）不发任何进度事件，空转不伪装成推进。逐只短窗翻页的基金在页抓取
/// 返回后额外带出 `fund` 页级明细（不改 `done`/`total`；单页不发）。
///
/// 写入见证（issue #1277）：每个实际写入点（行情报价落库 / 当周采样点落库 /
/// 名称随行刷新 / 基金净值落库 / 基金名称刷新）在落库成功后标记 [`WriteWitness`]
/// ——中途失败的运行结果统计随错误丢失，见证器由调用方持有（`&mut` 传入）存活，
/// 壳层据此把「实际写过」的失败收尾归一为证据（成败同判，见 `commands::sync`）。
/// 取数面注入（ADR-0121 / issue #1374 / ADR-0130 决策 2）：`bulk` 是新浪 `f_`
/// 场外基金批量面（名称与最新净值同面返回）+ 跨同步记忆的打包束（生产接 HTTP 层、
/// 测试注入桩，见 [`super::channels`]）。批量面只回答「这次刷新用几次请求」，不改变价格来源归属。
/// 货基判定确认闭包（issue #1563 / ADR-0126 决策 3 换源）由逐只刷新单元消费：
/// 未打标标的进逐只通道先确认、确认即退出采集链路。
// 五个逐标的抓取闭包（含货基判定确认）+ 取数面 + 会话 + 进度回调 + 写入见证
// 共 9 参：网络接缝逐通道注入使然（与 HTTP 层多主机请求同形），参数表就是
// 「本编排消费哪些外部通道」的清单（issue #1377 起日 K 与单请求全量净值两通道
// 归后台补全，不在本编排的参数表）。
#[allow(clippy::too_many_arguments)]
pub(super) async fn do_incremental_sync_with<Q, F, X, N, M, C, P>(
    session: &Q,
    fetch: &mut F,
    fetch_fx: &mut X,
    fetch_nav: &mut N,
    fetch_fund_name: &mut M,
    confirm_money_fund: &mut C,
    bulk: &mut BulkFetchSurfaces,
    progress: &mut P,
    witness: &mut WriteWitness,
) -> Result<SyncInstrumentInfoResult>
where
    // 作用域会话接缝（issue #1275 / #1412 async 形态）：读写库的唯一通道，
    // 签名层面取不到连接。
    Q: ScopedSession,
    F: FnMut(&[QuoteQuery]) -> FetchFuture<Vec<QuoteItem>> + Send,
    X: FnMut(&str) -> FetchFuture<Vec<KlineBar>> + Send,
    N: FnMut(&NavQuery) -> FetchFuture<NavPage> + Send,
    // 基金名称闭包（issue #827）：6 位代码 → 数据源权威名称；空串表示未取到
    // （不落库）。生产接基金详情通道，测试注入 mock。
    M: FnMut(&str) -> FetchFuture<String> + Send,
    // 货基判定确认闭包（issue #1563）：6 位代码 → 官方披露自报形态三态。
    C: FnMut(&str) -> FetchFuture<bool> + Send,
    // 进度回调闭包（issue #897 / ADR-0095；页级明细 issue #1061）：三字段载荷，
    // 逐有通道标的推进、基金深回填带页级明细；生产接事件发射（壳层接线），
    // 测试注入记录闭包。
    P: FnMut(SyncProgress) + Send,
{
    let held = session.with_connection(collect_instruments).await?;
    // 单次收集库内全部标的（一条 SQL，无持仓前置，issue #827），按投资域派生的
    // 价格通道分区（issue #1060，判定单点 `derive_price_channel`）：行情分区
    // 递「市场 + 代码」查报价与日 K；净值分区（fund 且 6 位真实代码，ADR-0038 决策 6）
    // 走历史净值通道；其余（恒定价格通道行：ADR-0126 决策 4 采集链路全豁免、
    // 批量面结构上也不覆盖它，#1451；手动报价通道与无来源行：债券/其他、市场
    // 未知自建行、名称充代码基金行等）计入跳过统计——各类统计天然同源。
    let quote_channel: Vec<&SyncInstrument> = held
        .iter()
        .filter(|i| i.channel == PriceChannel::Quote)
        .collect();
    // 净值分区不含恒定价格通道（ADR-0126 决策 4 / #1451）：恒定标的不进逐只
    // 刷新（批量面未覆盖不再对它回退）、不进进度分母、不计批量面缺口——为一
    // 个已知常量发请求收益为零。未打标的货基仍留在净值通道内，由逐只刷新的
    // 官方披露自报形态判定门确认即打标（issue #1563 / ADR-0126 决策 3 换源，
    // 见 [`refresh_one_fund_price`]），自下一轮同步起豁免。
    let funds: Vec<&SyncInstrument> = held
        .iter()
        .filter(|i| i.channel == PriceChannel::FundNav)
        .collect();
    // 不参与采集的行数（恒定价格行 + 手动报价通道与无来源行）：跳过统计的
    // 第一桶，与下方 skipped 汇总同源。
    let uncollected = held.len() - quote_channel.len() - funds.len();
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

    // 构造行情批量报价的查询单元（「市场 + 代码」，issue #1555）：编排不拼数据源
    // 查询键，键由报价通道内部构造。报价代码已归一化、与响应 f12 对齐；行情分区内
    // symbol 唯一（instruments 的 UNIQUE(symbol, instrument_type)），同代码不冲突。
    // 行情分区市场必可查：派生单点 `derive_price_channel` 只把 `quote_market` 的市场
    // 判成 Quote，绑定测试 `quote_channel_derivation_matches_secid_construction` 钉住
    // 这一不变量（同步域不再镜像市场能力判定，ADR-0103 / issue #1060 同款）；通道侧
    // 对无法构造键的市场另有防御兼底（不进请求）。
    let mut meta: HashMap<String, &SyncInstrument> = HashMap::new();
    let mut queryable: Vec<QuoteQuery> = Vec::new();
    for inst in &quote_channel {
        let code = quote_code(&inst.symbol);
        meta.insert(code.to_string(), inst);
        queryable.push(QuoteQuery {
            market: inst.market.clone(),
            code: code.to_string(),
        });
    }

    // 进度分母（issue #897 / ADR-0095）：有通道标的数 = 行情分区标的 + 净值分区
    // 基金（价格通道派生单点已保证行情分区市场可查、净值分区代码为 6 位真实代码；
    // 恒定价格行与无通道行不进分母，ADR-0126 决策 4）。收集与分区完成后立即发
    // total；total 为 0 不发任何进度事件。
    let total = queryable.len() + funds.len();
    if total > 0 {
        progress(SyncProgress::instrument(0, total));
    }
    let mut done = 0usize;

    // ① 查询并 upsert 现价（幂等：每标的一条 market_prices 覆盖更新，原行为不变），
    // 名称随行刷新（issue #827）：批量报价响应携带数据源权威名称，零额外请求，
    // 与价格解耦——停牌无价仍刷名称。报价 + 当周采样点直落合并为该标的一格
    //（issue #897；历史日 K 已移出本编排，ADR-0122 / issue #1377），停牌/查询无果
    // 照常推进。行情批量报价一次递出全部分区标的（issue #1560）：单次请求的批量
    // 承载量由取数层按实测请求行上限自行分批，编排不再按源相关批大小切割。
    let mut synced_codes: HashSet<String> = HashSet::new();
    let mut renamed = 0usize;
    // 批量报价是网络请求，在会话之外（await，issue #1412）；响应落库（名称
    // 随行刷新 + 现价 upsert）才短暂取一次连接（issue #1275）。查询键（腾讯
    // 市场前缀 / 美股交易所后缀）由通道内部构造（issue #1555 / #1560），编排只递
    // 「市场 + 代码」。
    // 库内行情分区为空则不调通道（零请求）：通道仍被调用会在无行情标的的
    // 用例现场多一次无谓请求（先例：批量面为空时不尝试）。
    if !queryable.is_empty() {
        let items = fetch(&queryable).await?;
        for item in &items {
            if let Some(inst) = meta.get(&item.code) {
                // 落库作业（门面作业形态，Send + 'static）：编现场的见证器 / 计数器
                // 进不了闭包，各写入点的成功标记经共享缓冲带出，await 之后回填——
                // 成败同判的见证语义逐字不变（后续步失败时前面已 autocommit 的
                // 写入仍计见证，issue #1277）。
                let marks = Arc::new(Mutex::new(Vec::new()));
                let sink = Arc::clone(&marks);
                let renamed_now = session
                    .with_connection({
                        let instrument_id = inst.instrument_id.clone();
                        let currency = inst.currency.clone();
                        let name = item.name.clone();
                        let price_cents = item.price_cents;
                        let price_date = item.price_date.clone();
                        move |conn| {
                            // 名称随行刷新（issue #827）：以数据源权威名称覆盖（仅实际变化才落库）。
                            if refresh_instrument_name(conn, &instrument_id, &name)? {
                                mark(&sink, StockMark::Renamed);
                            }
                            // 停牌/无效价（≤0）在取数层已解为 None，此处跳过、保留旧价。
                            if let Some(price_cents) = price_cents {
                                upsert_market_price(
                                    conn,
                                    &MarketPriceWrite {
                                        instrument_id: &instrument_id,
                                        price_cents,
                                        currency_code: &currency,
                                        // 场内现价时点 = 写入时刻、无净值日期语义（ADR-0036 /
                                        // ADR-0103 决策 4）；行情日期由当周采样点的
                                        // `trade_date` 承载（见下）。
                                        priced_at: &ledger_infra::db::now_iso(),
                                        nav_date: None,
                                        source: Some(TENCENT_PRICE_SOURCE),
                                    },
                                )?;
                                mark(&sink, StockMark::Priced);
                                // 当周采样点直落（ADR-0122 决策 2 / issue #1377）：现价刷新
                                // 已携带该标的当日有效报价，有历史序列者把当周点一并落库，
                                // 不另发逐只日 K 请求；无历史序列者不落（单点会冒充历史完整，
                                // 破坏后台补全的首刷判据）；同周同值零写入。采样日取
                                // **行情日期**——交易所当地交易日的日期部分，不做时区换算
                                //（ADR-0130 决策 5）：按北京时间切分会把美股周五的收盘记成
                                // 周六；取数层解不出日期时按北京日历日兜底。
                                let trade_date = price_date.clone().unwrap_or_else(|| {
                                    beijing_today().format("%Y-%m-%d").to_string()
                                });
                                super::history::land_current_week_point(
                                    conn,
                                    &instrument_id,
                                    &currency,
                                    &trade_date,
                                    price_cents,
                                )?;
                            }
                            Ok(())
                        }
                    })
                    .await;
                // 标记回填在错误判定之前：部分写入已 autocommit，见证照记（#1277）。
                for mark in marks
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .drain(..)
                {
                    match mark {
                        StockMark::Renamed => {
                            renamed += 1;
                            witness.mark_written();
                        }
                        StockMark::Priced => {
                            synced_codes.insert(item.code.clone());
                            witness.mark_written();
                        }
                    }
                }
                renamed_now?;
            }
        }
    }

    // 进度推进（issue #897）：每只有通道标的一格——现价 + 当周采样点 +
    // 名称随行刷新合并为一格，停牌/查询无果照常推进（不以成败计格）。
    // 历史日 K 回填已随 ADR-0122 / issue #1377 移出本编排（归后台补全），
    // 行情分区的逐只请求自此消失。
    for _ in &queryable {
        done += 1;
        progress(SyncProgress::instrument(done, total));
    }

    // ② 汇率 K 线回填 → FxRateHistory：共用单元（[`backfill_fx_pairs`]，issue
    // #1375 起与后台补全共用），仅非本位币币种对，与价格历史同期段采集。
    // 汇率落库不计入写入见证（issue #1277）：与成功路径的零写入判定同口径——
    // 只有价格或名称写入才发价格失效信号，汇率历史变化不在其列。
    backfill_fx_pairs(session, fetch_fx, held.iter().map(|s| s.currency.clone())).await?;

    // ③ 批量取数面（ADR-0121 / issue #1374 / ADR-0130 决策 2）：新浪 `f_` 场外
    // 基金批量面按本次现场的基金代码一次请求取回名称与最新净值（整次同步最多一次
    // 逻辑请求）——请求量自此不再随基金数线性增长；净值分区为空则零请求（无标的
    // 可刷，不白撞数据源）。失败 / 停用 / 未收录的标的在下方逐标的通道
    // fail-closed 兜底（缺口与失败分开统计，见 [`fetch_bulk_surfaces`]）。
    let mut bulk_gaps = 0usize;
    let fund_codes: Vec<String> = funds.iter().map(|fund| fund.symbol.clone()).collect();
    let bulk_data = if funds.is_empty() {
        BulkData::none()
    } else {
        fetch_bulk_surfaces(bulk, &fund_codes).await
    };

    // ④ 基金分区逐只（issue #897 逐只合并推进）：现价刷新（ADR-0122 决策 2，
    // 委托 [`refresh_one_fund_price`]——批量面命中整只零请求，未收录/降级退逐只
    // 短窗，issue #1377 起不再承担首刷与缺周点深补）+ 权威名称随行刷新
    //（issue #827）合并为该基金的一格；「已是最新（无新净值）」同样推进。名称与
    // 耗时随取数面改写：批量面命中即零请求（名称与最新净值同面返回，最新净值
    // 日期即「是否有新净值」的判据），未收录的标的退回既有逐标的通道（名称走
    // 基金详情通道、净值走 lsjz 短窗）。名称充代码行无通道：不进净值分区（不进
    // 分母、零请求），计入跳过（见上）。
    let mut fund_stats = FundSyncStats {
        synced: 0,
        skipped: 0,
        written: 0,
    };
    for fund in &funds {
        // 页级推进（issue #1061）：`done`/`total` 仍是标的级口径，页抓取返回后
        // 才带出本基金的页明细——抓取内部的退避/重试等待不产生推进。
        let code = fund.symbol.clone();
        // 覆盖面以名称字典为准（有名称即被面收录）：未收录 = 缺口，逐条回退逐标的
        // 通道补齐，不触发熔断。面已收录但净值表无该码 = 货基错位行（万份收益在
        // 单位净值位，ADR-0130 决策 6）——不是缺口，`latest_hint` 为 None 自然落
        // 逐只臂、由官方披露判定门确认收尾（issue #1565）。
        let covered = bulk_data.covers(&code);
        let latest_hint = bulk_data.nav_of(&code);
        if bulk_data.has_batch() && !covered {
            bulk_gaps += 1;
            tracing::debug!(
                code = %code,
                "场外基金批量面未收录该标的（新成立 / 已终止 / 清盘），逐只通道补齐"
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
            refresh_one_fund_price(
                session,
                fund,
                latest_hint,
                fetch_nav,
                confirm_money_fund,
                &mut fund_stats,
                &mut on_page,
            )
            .await?;
            // 净值实际落库才标记（「已是最新」不算写入，与 fund_stats.written 同判）。
            if fund_stats.written > written_before {
                witness.mark_written();
            }
        }
        // 名称随行刷新（issue #827；取数面随 ADR-0121 / ADR-0130 改写）：批量面
        // 命中即零请求（名称与净值同面返回，常数级请求量的一部分）；未收录的标的
        // 退回逐只详情通道——遇「确定性查无」降级为保留原名（ADR-0039 修订，
        // issue #1212）：基金可能已终止（搜索索引与档案通道都不再可达），但这不该
        // 打断整次同步；网络类失败仍按既有契约上抛。空名称 = 未取到，不落库。
        let name = match bulk_data.name_of(&code) {
            Some(name) => name.to_string(),
            None => {
                if bulk_data.has_batch() {
                    tracing::debug!(code = %code, "场外基金批量面未收录该标的，逐只名称通道补齐");
                }
                match fetch_fund_name(&fund.symbol).await {
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
        // 名称落库才短暂取一次连接（抓取在会话之外，issue #1275 / #1412 async 形态）。
        let instrument_id = fund.instrument_id.clone();
        if session
            .with_connection(move |conn| refresh_instrument_name(conn, &instrument_id, &name))
            .await?
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
    // 与恒定价格行（不进收集面，ADR-0126 决策 4）、停牌/查询无果/首刷查无净值等
    // 一并计入跳过。
    let skipped = uncollected + invalid + fund_stats.skipped;
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
pub(crate) fn beijing_today() -> NaiveDate {
    beijing_date(chrono::Utc::now())
}

/// [`beijing_today`] 的纯函数核：UTC 时刻加 8 小时后取日期部分即北京日历日
///（16:00 UTC 是北京午夜边界，边界语义由 `beijing_date_shifts_utc_by_plus_8h` 钉住）。
pub(super) fn beijing_date(now: chrono::DateTime<chrono::Utc>) -> NaiveDate {
    (now + chrono::Duration::hours(8)).date_naive()
}

/// 自然日窗口的开启判定（ADR-0122 决策 3「启动后延迟一次 + 此后每自然日各一次」的
/// 规则单点）：**同一北京日历日只开一次**，跨日（或从未跑过）即开。
///
/// 每日现价刷新与价格历史补全两条后台车道共用本判定——两条循环原先各自内联同一段
/// `last_round_date != Some(today)`（两处同体），规则散落；抽成纯函数后规则可在单测
/// 里直接钉住（同日不开、跨日开、首轮开），调度循环只负责把结果接上副作用。
///
/// 规则只回答「今天该不该开窗口」，不含任何副作用与时间读取——`today` 由调用方经
/// [`beijing_today`] 传入，纯函数可确定性测试。
pub(crate) fn daily_window_opens(last_run: Option<NaiveDate>, today: NaiveDate) -> bool {
    last_run != Some(today)
}

/// 近两年回填窗口起点：北京时间今天 − 2 年。A 股/港股交易日历以北京时间为准，
/// 起点精度只影响边界处至多多采一天的样本，周采样后无影响。股票日 K 与基金净值
/// 首刷窗口同此（#303，唯一实现）。
pub(super) fn two_years_ago(today: NaiveDate) -> NaiveDate {
    today
        .checked_sub_months(chrono::Months::new(24))
        .unwrap_or(today)
}

/// 近两年回填窗口（`YYYY-MM-DD` 形态，腾讯日 K 接口参数用，issue #1561）：
/// 起点 = 北京时间今天 − 2 年、终点 = 今天。生产通道束构造时取一次（每次同步
/// 一次的口径不变，与 [`kline_beg`] 同型）。
pub(crate) fn kline_window() -> (String, String) {
    let today = beijing_today();
    let beg = two_years_ago(today).format("%Y-%m-%d").to_string();
    (beg, today.format("%Y-%m-%d").to_string())
}

/// 近两年回填窗口起点（YYYYMMDD 形态，**东财**日 K 接口参数用——汇率 K 线腿
/// 随 ECB 换源退役前仍走东财，ADR-0130 决策 2 把汇率单列）。生产通道束构造时
/// 取一次（`channels::SyncFetchChannels::production_lane`，每次同步一次的口径不变）。
pub(crate) fn kline_beg() -> String {
    two_years_ago(beijing_today()).format("%Y%m%d").to_string()
}

/// 日线按 ISO 周降采样（ADR-0019）：每周取最后一个有报价交易日的 (日期, 收盘价)。
/// 输入日线按日期升序排序兑底（东财本就升序）；无效收盘价（≤0）与不可解析日期跳过；
/// 整周无有效报价则该周无点。周键见 [`week_monday`]。基金净值点共用本函数
///（单位净值即价格，ADR-0038 决策 3，fund_nav 攒齐全部净值点后一次降采样）。
pub(super) fn downsample_weekly(bars: &[KlineBar]) -> Vec<(String, f64)> {
    downsample_weekly_points(bars.iter().filter(|b| b.close > 0.0).filter_map(|b| {
        Some((
            NaiveDate::parse_from_str(&b.date, "%Y-%m-%d").ok()?,
            b.close,
        ))
    }))
}

/// 周采样核心（日线 / 基金净值 / ECB 汇率腿共用，issue #1542）：(日期, 数值) 点
/// 按 ISO 周降采样，每周取最后一个有报价交易日的 (trade_date, 数值)。无效数值
///（≤0）跳过；整周无有效报价则该周无点；输出按日期升序。周键见 [`week_monday`]。
pub(super) fn downsample_weekly_points<I>(points: I) -> Vec<(String, f64)>
where
    I: IntoIterator<Item = (NaiveDate, f64)>,
{
    let mut sorted: Vec<(NaiveDate, f64)> = points
        .into_iter()
        .filter(|(_, value)| *value > 0.0)
        .collect();
    sorted.sort_by_key(|(date, _)| *date);
    let mut by_week: BTreeMap<NaiveDate, (String, f64)> = BTreeMap::new();
    for (d, value) in sorted {
        // 升序遍历：后写入者即该周最后一个交易日。
        by_week.insert(week_monday(d), (d.format("%Y-%m-%d").to_string(), value));
    }
    by_week.into_values().collect()
}

/// 单只标的的周采样历史落库（ADR-0122 决策 8 / issue #1373）：日 K 回填与基金
/// 净值回填两条通道共用的「降采样 + 逐周 upsert」形体，不另写第二份采样落库。
/// 「整周覆盖」幂等由 `upsert_price_history` 的 UNIQUE 约束保证（同周重复获取
/// 零重复行）。返回本次落库的周点数（调用方可据此判定「是否实际写过」；既有
/// 调用点不消费该返回值，行为不变）。
///
/// `source` 由调用方按**实际取数源**声明（ADR-0130 决策 7）：场内日 K 走腾讯
/// （`TENCENT_PRICE_SOURCE`），场外基金净值走新浪（`SINA_PRICE_SOURCE`，
/// issue #1566 接线）——共用写入形体不再硬编码单一来源。
///
/// 本函数只写行、**不开事务**：调用方必须在**一只一个事务**里包住它
///（[`ensure_transaction`]），否则第 N 个周点写入失败会留下半根历史。三个现役
/// 调用点（行情分区日 K 回填、基金净值回填、基金现价刷新的缺周点补齐）都已如此
/// 接线；基金侧另有现价与历史同事务的需求，故事务边界留在调用方而非本函数。
pub(super) fn write_weekly_price_history(
    conn: &Connection,
    instrument_id: &str,
    currency: &str,
    bars: &[KlineBar],
    source: &str,
) -> Result<usize> {
    let points = downsample_weekly(bars);
    let count = points.len();
    for (trade_date, close) in points {
        upsert_price_history(
            conn,
            instrument_id,
            &trade_date,
            price_value_to_cents(close),
            currency,
            source,
        )?;
    }
    Ok(count)
}

/// 汇率 K 线回填（ADR-0019；issue #1375 起手动同步与后台补全共用单元）：给定
/// 标的币种集合中，仅非本位币币种对（与本位币相同的无需历史折算）按同期段
/// 采集、同周规则落库。汇率消费方含基金与股票的历史市值折算。
pub(super) async fn backfill_fx_pairs<Q, X>(
    session: &Q,
    fetch_fx: &mut X,
    currencies: impl Iterator<Item = String>,
) -> Result<()>
where
    Q: ScopedSession,
    X: FnMut(&str) -> FetchFuture<Vec<KlineBar>> + Send,
{
    let native = session.with_connection(default_currency_code).await?;
    let mut pairs: Vec<(String, String)> = currencies
        .map(|base| (base, native.clone()))
        .filter(|(base, quote)| base != quote)
        .collect();
    pairs.sort();
    pairs.dedup();
    for (base, quote) in &pairs {
        let pair = format!("{base}{quote}");
        // 汇率 K 线抓取在会话之外（await）；降采样落库才短暂取一次连接（issue #1275）。
        let bars = fetch_fx(&pair).await?;
        let (base, quote) = (base.clone(), quote.clone());
        session
            .with_connection(move |conn| {
                for (trade_date, rate) in downsample_weekly(&bars) {
                    upsert_fx_rate_history(
                        conn,
                        &base,
                        &quote,
                        &trade_date,
                        rate,
                        EASTMONEY_PRICE_SOURCE,
                    )?;
                }
                Ok(())
            })
            .await?;
    }
    Ok(())
}

/// 行情分区单只落库作业的写入点标记（issue #1412）：作业闭包是 `Send + 'static`
/// 形态，编排在现场的见证器与计数器进不了闭包——各写入点的成功标记经共享缓冲
/// 带出，`await` 之后回填（后续步失败时前面已 autocommit 的写入仍计见证，
/// issue #1277 语义逐字保持）。
enum StockMark {
    /// 名称随行刷新已落库。
    Renamed,
    /// 现价已落库。
    Priced,
}

/// [`StockMark`] 的作业侧落点：互斥体中毒时按原样取用（标记缓冲只在本作业与
/// 同一 `await` 链上的回填点之间传递，中毒仅发生于作业 panic，且 panic 后回填
/// 仍须尽力而为——错误本身沿编排 `Result` 上抛）。
fn mark(sink: &Mutex<Vec<StockMark>>, mark: StockMark) {
    sink.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(mark);
}

/// 该日所属 ISO 周的周一：降采样的周键，与 price_history / fx_rate_history 的
/// week_start 生成列（date(trade_date,'-6 days','weekday 1')）同口径。两侧恒等是
/// 「整周覆盖幂等」的隐式契约，由 `week_key_matches_sqlite_week_start_column` 测试绑定，
/// 防止周定义单侧调整后静默漂移。
pub(super) fn week_monday(d: NaiveDate) -> NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}
