use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ledger_infra::error::Result;
use ledger_investment::QuoteMarket;

use super::bulk::BulkFetchSurfaces;
use super::csrc::{CSRC_HOSTS, confirm_money_fund_form_from};
use super::fund::fetch_fund_quote_from;
use super::fund_nav::NavPoint;
use tokio::sync::Mutex as AsyncMutex;

use super::http::{
    ForegroundGuard, KlineBar, Pacer, build_client, lock_pacer, shared_pacer, wait_foreground_idle,
};
use super::incremental::{beijing_today, do_incremental_sync_with};
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use super::progress::SyncProgress;
use super::session::ScopedSession;
use super::sina_fund::{SINA_FUND_BATCH_HOSTS, SINA_FUND_HISTORY_HOSTS, fetch_fund_nav_history};
use super::tencent::{TENCENT_QUOTE_HOSTS, fetch_tencent_quotes};
use super::tencent_kline;

pub type FetchFuture<T> = Pin<Box<dyn Future<Output = Result<T>> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteQuery {
    pub market: QuoteMarket,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteItem {
    pub code: String,
    pub name: String,
    pub price_cents: Option<i64>,
    pub price_date: Option<String>,
}

pub type FetchQuotes = Box<dyn FnMut(&[QuoteQuery]) -> FetchFuture<Vec<QuoteItem>> + Send>;
pub type FetchKline = Box<dyn FnMut(&QuoteQuery) -> FetchFuture<Vec<KlineBar>> + Send>;
pub type FetchNavHistory = Box<dyn FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send>;
pub type FetchFundName = Box<dyn FnMut(&str) -> FetchFuture<String> + Send>;
pub type FetchMoneyFundForm = Box<dyn FnMut(&str) -> FetchFuture<bool> + Send>;

pub struct SyncFetchChannels {
    pub fetch_quotes: FetchQuotes,
    pub fetch_kline: FetchKline,
    pub fetch_nav_history: FetchNavHistory,
    pub fetch_fund_name: FetchFundName,
    pub confirm_money_fund_form: FetchMoneyFundForm,
    pub bulk: BulkFetchSurfaces,
}

#[derive(Debug, Clone, Default)]
pub(super) struct SyncFetchHosts {
    pub(super) quote: Vec<String>,
    pub(super) kline: Vec<String>,
    pub(super) fund_batch: Vec<String>,
    pub(super) fund_history: Vec<String>,
    pub(super) disclosure: Vec<String>,
}

impl SyncFetchChannels {
    pub fn production() -> Result<Self> {
        Self::production_lane(Lane::Foreground, production_hosts())
    }

    pub fn production_backfill() -> Result<Self> {
        Self::production_lane(Lane::Backfill, production_hosts())
    }

    pub(super) fn production_lane(lane: Lane, hosts: SyncFetchHosts) -> Result<Self> {
        Self::production_lane_on_pacer(lane, hosts, shared_pacer())
    }

    pub(super) fn production_lane_on_pacer(
        lane: Lane,
        hosts: SyncFetchHosts,
        pacer: Arc<AsyncMutex<Pacer>>,
    ) -> Result<Self> {
        let client = build_client()?;
        Ok(Self {
            fetch_quotes: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = hosts.quote.clone();
                Box::new(move |queries: &[QuoteQuery]| {
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
                let end = beijing_today().format("%Y-%m-%d").to_string();
                Box::new(move |query: &QuoteQuery| {
                    let symbol = tencent_kline::kline_symbol(query);
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    let end = end.clone();
                    Box::pin(async move {
                        let _foreground = lane.before_request().await;
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        tencent_kline::fetch_tencent_week_kline(
                            &client,
                            &mut pacer,
                            &hosts,
                            &symbol,
                            tencent_kline::KLINE_BEG,
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
                        fetch_fund_nav_history(&client, &mut pacer, &hosts, &code, None, None).await
                    })
                })
            },
            fetch_fund_name: {
                let pacer = pacer.clone();
                let batch_hosts = hosts.fund_batch.clone();
                let disclosure_hosts = hosts.disclosure.clone();
                Box::new(move |code: &str| {
                    let code = code.to_string();
                    let pacer = pacer.clone();
                    let batch_hosts = batch_hosts.clone();
                    let disclosure_hosts = disclosure_hosts.clone();
                    Box::pin(async move {
                        let _foreground = lane.before_request().await;
                        let client = build_client()?;
                        let mut pacer = lock_pacer(&pacer).await;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lane {
    Foreground,
    Backfill,
}

impl Lane {
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

    #[test]
    fn channels_bundle_drives_orchestration_end_to_end() {
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
