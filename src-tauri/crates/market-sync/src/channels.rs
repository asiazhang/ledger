//! 同步网络通道束（issue #1276）：六个抓取通道的打包形态与生产/测试换装接缝。
//!
//! 编排（[`super::incremental`]）消费六个抓取闭包（批量报价 / 日 K / 汇率 K /
//! 历史净值页 / 单请求全量净值 / 基金名称，见 `do_incremental_sync_with`）。
//! 本模块把它们打成**一个通道束**：生产经 [`SyncFetchChannels::production`]
//! 接 HTTP 层（复用主机池 / 重试 / 限流 pacer 与价格换算），测试把桩闭包装进
//! 同一结构注入命令壳（壳层 `SyncChannelsSlot` 管理态，issue #1276 的「命令壳
//! 同步网络通道注入接缝」）——「同步真实在途」由此可确定复现，编排与命令壳的
//! 锁形态在注入桩下原样运行，测试面与生产行为之间不再有形状差。
//!
//! 通道束只换装抓取闭包，不触其他接缝：会话（[`super::session`]）、进度发射
//! （[`super::progress`]）与编排本体对生产/测试零分叉。

use std::sync::{Arc, Mutex, MutexGuard};

use ledger_infra::error::{AppError, Result};

use super::fund::fetch_fund_quote_production;
use super::fund_nav::{LsjzPage, NavPoint, NavQuery, fetch_nav_full_series, fetch_nav_page};
use super::http::{
    KlineBar, Pacer, StockItem, build_client, fetch_fx_kline, fetch_kline, fetch_ulist,
};
use super::incremental::{do_incremental_sync_with, kline_beg};
use super::model::SyncInstrumentInfoResult;
use super::progress::SyncProgress;
use super::session::ScopedSession;

/// 取一次限流 pacer（毒化映射 [`AppError::Io`]：串行使用下锁竞争不存在，
/// 毒化仅发生于抓取 panic，属基础设施失败）。
fn lock_pacer(pacer: &Mutex<Pacer>) -> Result<MutexGuard<'_, Pacer>> {
    pacer
        .lock()
        .map_err(|e| AppError::Io(format!("限流器互斥体损坏: {e}")))
}

/// 抓取通道闭包的统一形态（`Box<dyn FnMut>` 别名，降低束字段签名复杂度；限
/// `Send` 以便整束经互斥体跨线程交接）。
pub type FetchUlist = Box<dyn FnMut(&str) -> Result<Vec<StockItem>> + Send>;
/// 日 K / 汇率 K 抓取通道闭包形态（两通道同签名，别名共用）。
pub type FetchKline = Box<dyn FnMut(&str) -> Result<Vec<KlineBar>> + Send>;
/// 历史净值页抓取通道闭包形态。
pub type FetchNavPage = Box<dyn FnMut(&NavQuery) -> Result<LsjzPage> + Send>;
/// 单请求全量净值抓取通道闭包形态（issue #1062 首刷深回填通道）。
pub type FetchNavFull = Box<dyn FnMut(&str) -> Result<Vec<NavPoint>> + Send>;
/// 基金详情名称抓取通道闭包形态（issue #827）。
pub type FetchFundName = Box<dyn FnMut(&str) -> Result<String> + Send>;

/// 六个抓取通道的打包束：闭包签名与编排注入点逐一同形。生产实现共享一个
/// HTTP client 与限流 pacer（`Arc<Mutex<_>>` 内部可变，串行使用下与既有局部
/// `RefCell` 共享语义一致）；测试实现为注入桩。
pub struct SyncFetchChannels {
    /// 批量报价（东财 ulist）：secid 逗号串 → 报价条目。
    pub fetch_ulist: FetchUlist,
    /// 日 K（近两年日线）：secid → 日线序列。
    pub fetch_kline: FetchKline,
    /// 汇率 K 线：币种对（如 `USDCNY`）→ 日线序列。
    pub fetch_fx: FetchKline,
    /// 历史净值页（lsjz 分页）：查询 → 单页净值。
    pub fetch_nav: FetchNavPage,
    /// 单请求全量净值（首刷深回填，issue #1062）：代码 → 整只历史单位净值。
    pub fetch_nav_full: FetchNavFull,
    /// 基金详情名称（issue #827）：代码 → 数据源权威名称。
    pub fetch_fund_name: FetchFundName,
}

impl SyncFetchChannels {
    /// 生产通道束：六个闭包接 HTTP 层（`build_client` 主机池 / 重试；pacer 以
    /// `Arc<Mutex<_>>` 共享，保证全部请求之间仍保持统一的限速间隔——与先前
    /// `do_incremental_sync` 局部闭包的共享语义逐字节一致）。回填窗口起点在
    /// 束构造时取一次（与先前每次同步取一次同口径）。
    pub fn production() -> Result<Self> {
        let client = build_client()?;
        let pacer = Arc::new(Mutex::new(Pacer::default()));
        let beg = kline_beg();
        Ok(Self {
            fetch_ulist: {
                let client = client.clone();
                let pacer = pacer.clone();
                Box::new(move |secids: &str| {
                    let mut pacer = lock_pacer(&pacer)?;
                    fetch_ulist(&client, &mut pacer, secids)
                })
            },
            fetch_kline: {
                let client = client.clone();
                let pacer = pacer.clone();
                let beg = beg.clone();
                Box::new(move |secid: &str| {
                    let mut pacer = lock_pacer(&pacer)?;
                    fetch_kline(&client, &mut pacer, secid, &beg)
                })
            },
            fetch_fx: {
                let client = client.clone();
                let pacer = pacer.clone();
                let beg = beg.clone();
                Box::new(move |pair: &str| {
                    let mut pacer = lock_pacer(&pacer)?;
                    fetch_fx_kline(&client, &mut pacer, pair, &beg)
                })
            },
            fetch_nav: {
                let client = client.clone();
                let pacer = pacer.clone();
                Box::new(move |query: &NavQuery| {
                    let mut pacer = lock_pacer(&pacer)?;
                    fetch_nav_page(&client, &mut pacer, query)
                })
            },
            fetch_nav_full: {
                let client = client.clone();
                let pacer = pacer.clone();
                Box::new(move |code: &str| {
                    let mut pacer = lock_pacer(&pacer)?;
                    fetch_nav_full_series(&client, &mut pacer, code)
                })
            },
            fetch_fund_name: Box::new(move |code: &str| {
                fetch_fund_quote_production(code).map(|quote| quote.name)
            }),
        })
    }
}

/// 经通道束驱动标的信息同步编排：把束内六个闭包拆交给
/// [`do_incremental_sync_with`](super::incremental::do_incremental_sync_with)
/// （编排本体单点，签名不变）。命令壳经本入口跑同步——生产束
/// （[`SyncFetchChannels::production`]）与测试注入束共用，锁形态与编排路径
/// 零分叉。
pub fn do_incremental_sync_channels<Q, P>(
    session: &Q,
    channels: &mut SyncFetchChannels,
    progress: &mut P,
) -> Result<SyncInstrumentInfoResult>
where
    Q: ScopedSession,
    P: FnMut(SyncProgress),
{
    do_incremental_sync_with(
        session,
        &mut channels.fetch_ulist,
        &mut channels.fetch_kline,
        &mut channels.fetch_fx,
        &mut channels.fetch_nav,
        &mut channels.fetch_nav_full,
        &mut channels.fetch_fund_name,
        progress,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// 注入桩通道束（全通道空应答的最小形状）：通道束的测试构造面与生产
    /// `production` 同型——命令壳换装时对两侧零分叉。
    fn stub_channels() -> SyncFetchChannels {
        SyncFetchChannels {
            fetch_ulist: Box::new(|_| Ok(vec![])),
            fetch_kline: Box::new(|_| Ok(vec![])),
            fetch_fx: Box::new(|_| Ok(vec![])),
            fetch_nav: Box::new(|_| {
                Ok(LsjzPage {
                    points: vec![],
                    total: 0,
                    blocked: false,
                })
            }),
            fetch_nav_full: Box::new(|_| Ok(vec![])),
            fetch_fund_name: Box::new(|_| Ok(String::new())),
        }
    }

    struct Passthrough<'a>(&'a Connection);

    impl ScopedSession for Passthrough<'_> {
        fn with_connection<R, F>(&self, use_connection: F) -> Result<R>
        where
            F: FnOnce(&Connection) -> Result<R>,
        {
            use_connection(self.0)
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
        let result =
            do_incremental_sync_channels(&Passthrough(&conn), &mut channels, &mut progress)
                .expect("空库同步应成功返回");
        assert_eq!(result.synced, 0);
        assert_eq!(result.skipped, 0);
        assert_eq!(result.message, "暂无标的可同步");
    }
}
