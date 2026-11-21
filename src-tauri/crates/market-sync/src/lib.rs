#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

mod bulk;
mod channels;
mod csrc;
mod daily_refresh;
mod ecb;
mod fund;
mod fund_backfill;
mod fund_nav;
mod fund_price_refresh;
mod fx;
mod fx_daily;
mod history;
mod http;
mod incremental;
mod lane;
mod model;
mod persist;
mod progress;
mod session;
mod sina_fund;
mod source_tail;
mod stock;
mod tencent;
mod tencent_kline;
mod weekly;

#[cfg(test)]
mod tests;

pub use bulk::{
    BULK_DISABLE_PERIOD, BULK_FAILURE_THRESHOLD, BulkFetchCircuit, BulkFetchSurfaces, BulkNavPoint,
    FundBatch, FundNameDictionary, FundNavTable,
};
pub use channels::{
    FetchFundName, FetchKline, FetchMoneyFundForm, FetchNavHistory, FetchQuotes, QuoteItem,
    QuoteQuery, SyncFetchChannels, do_incremental_sync_channels,
};
pub use daily_refresh::{
    DailyPriceRefreshChannelsSlot, start_daily_price_refresh, start_daily_price_refresh_with,
};
pub use ecb::EcbDayRates;
pub use fund::fetch_fund_quote_production;
pub use fund_nav::NavPoint;
pub use fx::{
    FX_SYNC_GATE, FxSyncChannels, FxSyncGate, FxSyncReport, sync_fx_rates, sync_fx_rates_guarded,
    sync_fx_rates_with_progress,
};
pub use fx_daily::{DailyFxSyncChannelsSlot, start_daily_fx_sync, start_daily_fx_sync_with};
pub use history::{BackfillChannelsSlot, start_history_backfill, start_history_backfill_with};
pub use http::KlineBar;
pub use lane::LaneTimings;
pub use model::{SyncInstrumentInfoResult, WriteWitness};
pub use persist::FxPersistReport;
pub use progress::{
    BackfillProgressEmitter, FX_SYNC_PROGRESS, FxProgressEmitter, FxSyncProgress, FxSyncStage,
    HISTORY_BACKFILL_PROGRESS, INSTRUMENT_SYNC_PROGRESS, ProgressEmitter, SyncProgress,
};
pub use session::{FacadeWriteSession, ScopedSession};
pub use stock::fetch_stock_quote_production;
