//! 同步网络通道束（issue #1276）：五个逐标的抓取通道 + 一个批量取数面
//!（ADR-0121 / issue #1374 / ADR-0130 决策 2）的打包形态与生产/测试换装接缝。
//!
//! 束内闭包：批量报价 / 日 K / 新浪单只全历史 / 基金名称 /
//! 货基判定确认（issue #1563 换源，由现价刷新与历史补全两编排消费）五个，
//! 外加一个批量取数面（新浪 `f_` 面：名称与最新净值同面返回；issue #1565 换源）。
//! 增量编排（[`super::incremental`]）消费批量报价 / 新浪单只全历史
//!（issue #1571 起与历史补全同通道）/ 基金名称三闭包 + 批量面 + 货基确认，
//! 见 `do_incremental_sync_with`；日 K 通道归价格历史后台补全（issue #1377）。
//! 汇率序列的采集腿已随 #1551 换 ECB 退役（[`super::fx`] 的 `FxSyncChannels`
//! 是独立的汇率通道束，不经本束）。
//! 基金名称闭包走按代码取价编排（新浪批量面 + 官方披露，issue #1568 换源）。
//! 本模块把它们打成**一个通道束**：生产经 [`SyncFetchChannels::production`]
//! 接 HTTP 层（复用主机池 / 重试 / 限流 pacer 与价格换算），测试把桩闭包装进
//! 同一结构注入命令壳（壳层 `SyncChannelsSlot` 管理态，issue #1276 的「命令壳
//! 同步网络通道注入接缝」）——「同步真实在途」由此可确定复现，编排与命令壳的
//! 锁形态在注入桩下原样运行，测试面与生产行为之间不再有形状差。
//!
//! 抓取闭包为 **async 形态**（ADR-0125 决策 5 / issue #1412）：返回装箱 future、
//! 网络等待以 `await` 表达，#1411 的过渡同步桥不再经本模块。闭包入参为引用、
//! future 需 `'static`，实现侧在构造 future 前把入参拷为自有数据。
//!
//! 通道束只换装抓取闭包，不触其他接缝：会话（[`super::session`]）、进度发射
//! （[`super::progress`]）与编排本体对生产/测试零分叉。
//!
//! 车道（issue #1375 额度让路）：生产束分前台（[`SyncFetchChannels::production`]，
//! 手动同步等用户动作）与后台（[`SyncFetchChannels::production_backfill`]，价格
//! 历史补全）两条——同一进程级全局限速器（[`super::http::shared_pacer`]）串行
//! 两道车流的相邻请求，前台请求在途时后台车道让行（[`super::http::wait_foreground_idle`]
//! 等归零再发），前台对数据源的响应时间不被后台拖慢。

use std::future::Future;
use std::pin::Pin;

use ledger_infra::error::Result;
use ledger_investment::QuoteMarket;

use super::bulk::BulkFetchSurfaces;
use super::csrc::{CSRC_HOSTS, confirm_money_fund_form_from};
use super::fund::fetch_fund_quote_from;
use super::fund_nav::NavPoint;
use super::http::{
    ForegroundGuard, KlineBar, build_client, lock_pacer, shared_pacer, wait_foreground_idle,
};
use super::incremental::{do_incremental_sync_with, kline_window};
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use super::progress::SyncProgress;
use super::session::ScopedSession;
use super::sina_fund::{SINA_FUND_BATCH_HOSTS, SINA_FUND_HISTORY_HOSTS, fetch_fund_nav_history};
use super::tencent::{TENCENT_QUOTE_HOSTS, fetch_tencent_quotes};
use super::tencent_kline;

/// 抓取通道 future 的装箱形态：网络等待以 `await` 表达（ADR-0125 决策 5 /
/// issue #1412）；限 `Send` 以便整束经互斥体跨线程交接。
pub type FetchFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

/// 「市场 + 代码」查询单元（issue #1555 批量报价 / issue #1556 日 K）：编排只递
/// 「市场 + 代码」，数据源查询键（腾讯报价键 / 腾讯 K 线键等）由
/// 各通道在内部构造——换源只改通道实现，编排零改动。`market` 是可路由市场子集
/// [`QuoteMarket`]（市场闭集去掉 unknown，issue #1673）——不可路由市场在类型上
/// 无法进入查询单元，数据源键构造因此是全函数；`code` 是响应回显形态的裸代码
///（如 `600519` / `00700`，已去市场后缀）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteQuery {
    pub market: QuoteMarket,
    pub code: String,
}

/// 场内批量报价条目（通道束注入接缝的公开面，issue #1560）：编排只消费这四个
/// 成员——代码、数据源权威名称、价格（万分之一元，ADR-0038）与行情日期
///（交易所当地交易日，ADR-0130 决策 5）。数据源私有字段（类型码 / 币种 /
/// 精确市场）不进本形状，换源只改投影端（见 `tencent::TencentQuote` 的投影）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteItem {
    pub code: String,
    pub name: String,
    pub price_cents: Option<i64>,
    pub price_date: Option<String>,
}

/// 抓取通道闭包的统一形态（`Box<dyn FnMut>` 别名，降低束字段签名复杂度）。
pub type FetchQuotes = Box<dyn FnMut(&[QuoteQuery]) -> FetchFuture<Vec<QuoteItem>> + Send>;
/// 日 K（近两年日线）抓取通道闭包形态：「市场 + 代码」查询单元 → 日线序列；
/// 查询键（腾讯 K 线键，issue #1561 接线）由通道内部构造（issue #1556 起编排
/// 不拼数据源键，与批量报价通道同形）。
pub type FetchKline = Box<dyn FnMut(&QuoteQuery) -> FetchFuture<Vec<KlineBar>> + Send>;
/// 单只全历史净值抓取通道闭包形态（新浪全历史面，issue #1566 接线）：代码 →
/// 整只历史单位净值（含已终止基金）；窗口语义（首刷近两年 / 水位次日增量）由
/// 消费方本地裁剪，通道不携带窗口参数。现价刷新逐只回退与价格历史后台补全
/// 共用本通道（issue #1571：逐只分页通道随 lsjz 换源退役）。
pub type FetchNavHistory = Box<dyn FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send>;
/// 基金详情名称抓取通道闭包形态（issue #827）。
pub type FetchFundName = Box<dyn FnMut(&str) -> FetchFuture<String> + Send>;
/// 货基判定确认通道闭包形态（issue #1563 / ADR-0126 决策 3 换源）：6 位代码 →
/// 官方披露自报形态确认。三态：`Ok(true)` = 货基形态确认；`Ok(false)` = 缺信号
///（不是「不是恒定标的」的反证）；`Err` = 披露源不可信（本轮不落任何价格）。
pub type FetchMoneyFundForm = Box<dyn FnMut(&str) -> FetchFuture<bool> + Send>;

/// 五个逐标的抓取通道 + 一个批量取数面的打包束：闭包签名与编排注入点逐一同形。
/// 生产实现共享一个异步 HTTP client 与限流 pacer（`Arc<tokio::sync::Mutex<_>>`
/// 内部可变、跨 `.await` 持有，串行语义与既有守卫一致，批量面同样经它限速）；
/// 测试实现为注入桩。
pub struct SyncFetchChannels {
    /// 批量报价（腾讯行情，issue #1560）：一批「市场 + 代码」查询单元 → 报价条目；
    /// 查询键（市场前缀/交易所后缀）由通道内部构造（issue #1555）。
    pub fetch_quotes: FetchQuotes,
    /// 日 K（近两年日线）：「市场 + 代码」查询单元 → 日线序列；查询键（腾讯
    /// K 线键）由通道内部构造（issue #1556 接缝，issue #1561 接线腾讯 K 线）。
    pub fetch_kline: FetchKline,
    /// 单只全历史净值（新浪全历史面，issue #1566 接线）：代码 → 整只历史
    /// 单位净值（含已终止基金）；价格历史后台补全（首刷深回填与缺周点补齐）
    /// 与现价刷新逐只回退共用（issue #1571：逐只分页通道随 lsjz 换源退役）。
    pub fetch_nav_history: FetchNavHistory,
    /// 基金详情名称（issue #827）：代码 → 数据源权威名称。主机取注入面的
    /// 新浪批量面 + 证监会披露面两面（三臂编排：批量面未收录 → 披露兑底，
    /// issue #1674 接注入面）。
    pub fetch_fund_name: FetchFundName,
    /// 货基判定确认（issue #1563 / ADR-0126 决策 3 换源）：代码 → 官方披露
    /// 自报形态确认。主机取注入面的证监会披露面（issue #1674 接注入面）。
    /// 逐只刷新与历史首刷两确认点消费；判定不进日常热路径——确认后标的退出
    /// 采集链路，不再产生逐轮请求（ADR-0126 决策 3/4）。
    pub confirm_money_fund_form: FetchMoneyFundForm,
    /// 批量取数面（ADR-0121 / issue #1374 / ADR-0130 决策 2）：新浪 `f_` 面把
    /// 名称与最新净值同面返回（issue #1565 换源）+ 跨同步记忆。
    pub bulk: BulkFetchSurfaces,
}

/// 生产通道束的取数主机注入面（测试用，ADR-0130 取数面 + issue #1674 五面）：
/// 五面的主机列表各具名——相邻同型 `Vec<String>` 位置参数可互换编译（静默错
/// 路由），具名结构让「哪一面用哪个本地主机」在调用点自证。束内六条闭包的
/// 主机一律取自本面（`fetch_fund_name` 消费批量面 + 披露面，
/// `confirm_money_fund_form` 消费披露面），闭包体不写死主机——生产常量与测试
/// 注入同形，钉接线测试逐条断言请求打到注入面指定的主机（见
/// [`SyncFetchChannels::production_lane`] 的六条接线 ↔ 六条测试对照清单）。
#[derive(Debug, Clone, Default)]
pub(super) struct SyncFetchHosts {
    /// 腾讯行情批量报价主机。
    pub(super) quote: Vec<String>,
    /// 腾讯日 K 主机。
    pub(super) kline: Vec<String>,
    /// 新浪场外基金批量面主机（名称 + 最新净值同面，issue #1565）。
    pub(super) fund_batch: Vec<String>,
    /// 新浪场外基金单只全历史面主机（历史补全，issue #1566）。
    pub(super) fund_history: Vec<String>,
    /// 证监会基金电子披露面主机（货基形态确认与批量面未收录的披露兑底，
    /// issue #1674）。
    pub(super) disclosure: Vec<String>,
}

impl SyncFetchChannels {
    /// 生产通道束（前台车道，手动同步等用户动作）：五个闭包接 HTTP 层
    ///（`build_client` 主机池 / 重试；pacer 取**进程级全局限速器**单点，issue
    /// #1375——与后台补全共用同一份数据源额度）。回填窗口起点在束构造时取
    /// 一次（与先前每次同步取一次同口径）。
    pub fn production() -> Result<Self> {
        Self::production_lane(Lane::Foreground, production_hosts())
    }

    /// 生产通道束（后台车道，issue #1375 价格历史补全）：同一全局限速器，但
    /// 请求前不占前台在途计数、反而**让行**——前台请求在途时后台等归零再发，
    /// 用户动作优先于后台补全。束形状与前台车道完全一致（编排消费零分叉）。
    pub fn production_backfill() -> Result<Self> {
        Self::production_lane(Lane::Backfill, production_hosts())
    }

    /// 生产通道束构造本体：`hosts` 携带五面注入主机（场内报价 / 场内日 K /
    /// 新浪基金批量面 / 新浪单只历史面 / 证监会披露面）。生产经
    /// [`production_hosts`] 传取数单元单点常量；测试注入本地 HTTP 服务，驱动
    /// **生产束**钉住全部六条接线——「六条接线 ↔ 六条钉接线测试」对照清单
    /// （删除任一接线即红，ADR-0087）：
    ///
    /// | 束内闭包 | 打到的注入面 | 钉接线测试 |
    /// |---|---|---|
    /// | `fetch_quotes` | 场内报价 | `production_quote_channel_requests_tencent_batch_endpoint`（issue #1560） |
    /// | `fetch_kline` | 场内日 K | `production_backfill_channel_lands_history_via_tencent_kline`（issue #1561） |
    /// | `bulk`（批量取数面） | 新浪基金批量面 | `production_fund_batch_channel_requests_sina_batch_endpoint`（issue #1565） |
    /// | `fetch_nav_history` | 新浪单只历史面 | `production_backfill_channel_lands_fund_history_via_sina`（issue #1566） |
    /// | `fetch_fund_name` | 新浪基金批量面 + 证监会披露面（批量面未收录 → 披露兑底双主机） | `production_fund_name_channel_falls_back_to_disclosure_host`（issue #1674） |
    /// | `confirm_money_fund_form` | 证监会披露面 | `production_confirm_channel_requests_disclosure_host`（issue #1674） |
    pub(super) fn production_lane(lane: Lane, hosts: SyncFetchHosts) -> Result<Self> {
        let client = build_client()?;
        let pacer = shared_pacer();
        Ok(Self {
            fetch_quotes: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = hosts.quote.clone();
                Box::new(move |queries: &[QuoteQuery]| {
                    // 查询键（腾讯市场前缀 / 美股交易所后缀）在通道内部构造
                    //（issue #1555 / #1558）：编排只递「市场 + 代码」；批量承载量
                    // 由取数层按实测请求行上限自行分批（issue #1558）。
                    let queries = queries.to_vec();
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let _foreground = lane.before_request().await;
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        let quotes =
                            fetch_tencent_quotes(&client, &mut pacer, &hosts, &queries).await?;
                        Ok(quotes.into_iter().map(|q| q.into_quote_item()).collect())
                    })
                })
            },
            fetch_kline: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = hosts.kline.clone();
                // 近两年窗口在束构造时取一次（与汇率腿同口径）。
                let (beg, end) = kline_window();
                Box::new(move |query: &QuoteQuery| {
                    // 查询键（腾讯 K 线键）在通道内部构造（issue #1559 / #1561）：
                    // 编排只递「市场 + 代码」。查询单元市场是可路由子集
                    //（issue #1673），键构造是全函数、无「构造不出键」分支。
                    let symbol = tencent_kline::kline_symbol(query);
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    let beg = beg.clone();
                    let end = end.clone();
                    Box::pin(async move {
                        let _foreground = lane.before_request().await;
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        tencent_kline::fetch_tencent_day_kline(
                            &client,
                            &mut pacer,
                            &hosts,
                            &symbol,
                            &beg,
                            &end,
                            tencent_kline::KLINE_COUNT,
                        )
                        .await
                    })
                })
            },
            fetch_nav_history: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = hosts.fund_history.clone();
                Box::new(move |code: &str| {
                    let code = code.to_string();
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let _foreground = lane.before_request().await;
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        // 窗口参数不传（None = 不限）：整只历史一次取全，首刷近
                        // 两年 / 水位次日增量的窗口语义由消费方本地裁剪——不依赖
                        // 服务端窗口过滤行为（issue #1566）。
                        fetch_fund_nav_history(&client, &mut pacer, &hosts, &code, None, None).await
                    })
                })
            },
            fetch_fund_name: {
                let client = client.clone();
                let pacer = pacer.clone();
                let batch_hosts = hosts.fund_batch.clone();
                let disclosure_hosts = hosts.disclosure.clone();
                Box::new(move |code: &str| {
                    let code = code.to_string();
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let batch_hosts = batch_hosts.clone();
                    let disclosure_hosts = disclosure_hosts.clone();
                    Box::pin(async move {
                        let _foreground = lane.before_request().await;
                        let mut pacer = lock_pacer(&pacer).await;
                        // 三臂取数编排（新浪批量面 → 官方披露兑底，issue #1568）：
                        // 两面主机都取注入面（fund_batch / disclosure），与生产
                        // 常量同形、与共享限速器同额度（issue #1674：本地限速器
                        // 归零、写死主机归零）。
                        let batch: Vec<&str> = batch_hosts.iter().map(String::as_str).collect();
                        let disclosure: Vec<&str> =
                            disclosure_hosts.iter().map(String::as_str).collect();
                        fetch_fund_quote_from(&client, &mut pacer, &code, &batch, &disclosure)
                            .await
                            .map(|quote| quote.name)
                    })
                })
            },
            confirm_money_fund_form: {
                let client = client.clone();
                let pacer = pacer.clone();
                let disclosure_hosts = hosts.disclosure.clone();
                Box::new(move |code: &str| {
                    let code = code.to_string();
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let disclosure_hosts = disclosure_hosts.clone();
                    Box::pin(async move {
                        let _foreground = lane.before_request().await;
                        let mut pacer = lock_pacer(&pacer).await;
                        // 判定确认只问官方披露（issue #1563 / ADR-0126 决策 3）：
                        // 主机取注入面 disclosure 面（issue #1674），与生产常量同形。
                        let disclosure: Vec<&str> =
                            disclosure_hosts.iter().map(String::as_str).collect();
                        confirm_money_fund_form_from(&client, &mut pacer, &code, &disclosure).await
                    })
                })
            },
            bulk: BulkFetchSurfaces::production(&client, pacer, hosts.fund_batch.clone()),
        })
    }
}

/// 五条取数面的生产主机（发送单元单点常量的拥有副本）：取数单元单点常量
/// [`TENCENT_QUOTE_HOSTS`]、[`tencent_kline::TENCENT_KLINE_HOSTS`]、
/// [`SINA_FUND_BATCH_HOSTS`]、[`SINA_FUND_HISTORY_HOSTS`] 与 [`CSRC_HOSTS`] 的
/// `Vec<String>` 形态，供通道束构造持有；测试注入本地 HTTP 服务地址替换它们。
/// 按代码查询（行情接入的查询半边，束外）取的也是同一组生产常量
///（[`super::fund::fetch_fund_quote`] / [`super::stock::fetch_stock_quote`] 的
/// 生产入口），主机单点不因束内束外而分叉。
fn production_hosts() -> SyncFetchHosts {
    SyncFetchHosts {
        quote: TENCENT_QUOTE_HOSTS
            .iter()
            .map(|host| host.to_string())
            .collect(),
        kline: tencent_kline::TENCENT_KLINE_HOSTS
            .iter()
            .map(|host| host.to_string())
            .collect(),
        fund_batch: SINA_FUND_BATCH_HOSTS
            .iter()
            .map(|host| host.to_string())
            .collect(),
        fund_history: SINA_FUND_HISTORY_HOSTS
            .iter()
            .map(|host| host.to_string())
            .collect(),
        disclosure: CSRC_HOSTS.iter().map(|host| host.to_string()).collect(),
    }
}

/// 通道车道（issue #1375）：前台请求在途计数（[`ForegroundGuard`]）让后台让行；
/// 后台请求发前等在途归零。闭包请求前的统一前置动作收在 [`Lane::before_request`]：
/// 前台车道返回在途守卫（RAII，闭包返回自动释放），后台车道等待归零、返回 None。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    Foreground,
    Backfill,
}

impl Lane {
    /// 请求前置动作：前台 = 标记在途；后台 = 让行等待归零（异步睡眠，ADR-0125
    /// 决策 6）。返回的守卫必须以 `let _foreground = …` 绑定存活到请求结束
    ///（`let _ = …` 会立即丢弃）。
    async fn before_request(&self) -> Option<ForegroundGuard> {
        match self {
            Lane::Foreground => Some(ForegroundGuard::enter()),
            Lane::Backfill => {
                wait_foreground_idle().await;
                None
            }
        }
    }
}

/// 经通道束驱动标的信息同步编排：把束内现价刷新所需的闭包拆交给
/// [`do_incremental_sync_with`](super::incremental::do_incremental_sync_with)
/// （编排本体单点，另透传写入见证，issue #1277）。命令壳经本入口跑同步——
/// 生产束（[`SyncFetchChannels::production`]）与测试注入束共用，锁形态与
/// 编排路径零分叉。日 K 通道不进现价刷新编排（issue #1377 现价与历史解耦）：
/// 束内保留供价格历史后台补全消费（issue #1561）；新浪单只全历史通道自
/// issue #1571 起为现价刷新逐只回退与后台补全共用（逐只分页通道随 lsjz 换源
/// 退役）。
pub async fn do_incremental_sync_channels<Q, P>(
    session: &Q,
    channels: &mut SyncFetchChannels,
    progress: &mut P,
    witness: &mut WriteWitness,
) -> Result<SyncInstrumentInfoResult>
where
    Q: ScopedSession,
    P: FnMut(SyncProgress) + Send,
{
    do_incremental_sync_with(
        session,
        &mut channels.fetch_quotes,
        &mut channels.fetch_nav_history,
        &mut channels.fetch_fund_name,
        &mut channels.confirm_money_fund_form,
        &mut channels.bulk,
        progress,
        witness,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 注入桩通道束（全通道空应答的最小形状）：通道束的测试构造面与生产
    /// `production` 同型——命令壳换装时对两侧零分叉。
    fn stub_channels() -> SyncFetchChannels {
        SyncFetchChannels {
            fetch_quotes: Box::new(|_| Box::pin(async { Ok(vec![]) })),
            fetch_kline: Box::new(|_| Box::pin(async { Ok(vec![]) })),
            fetch_nav_history: Box::new(|_| Box::pin(async { Ok(vec![]) })),
            fetch_fund_name: Box::new(|_| Box::pin(async { Ok(String::new()) })),
            confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
            bulk: BulkFetchSurfaces::absent(),
        }
    }

    /// 直通会话（测试态）：作业闭包在 `await` 点内联完成——future 立即就绪，
    /// 连接引用不进 future 状态（`ready` 的载荷是业务结果）。
    struct Passthrough<'a>(&'a Connection);

    impl ScopedSession for Passthrough<'_> {
        fn with_connection<R, F>(&self, use_connection: F) -> impl Future<Output = Result<R>> + Send
        where
            F: FnOnce(&Connection) -> Result<R> + Send + 'static,
            R: Send + 'static,
        {
            std::future::ready(use_connection(self.0))
        }
    }

    /// 通道束换装编排的 plumbing 钉（issue #1276）：经 `do_incremental_sync_
    /// channels` 驱动一次空库同步，结果与直接调编排本体同形（空库明确提示、
    /// 不报错）——束 → 编排的拆交不吞闭包、不改返回。
    #[test]
    fn channels_bundle_drives_orchestration_end_to_end() {
        // 域 crate 测试经 dev-dependency 消费壳层测试工厂（既有域测试同款）。
        let conn = tauri_app_lib::test_support::open();
        let mut channels = stub_channels();
        let mut progress = |_| {};
        let mut witness = WriteWitness::default();
        let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
            &Passthrough(&conn),
            &mut channels,
            &mut progress,
            &mut witness,
        ))
        .expect("空库同步应成功返回");
        assert_eq!(result.synced, 0);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.message, "暂无标的可同步");
        assert!(!witness.any_written(), "空库同步零写入，见证器不应标记");
    }
}
