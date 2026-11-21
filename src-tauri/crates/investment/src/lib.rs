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

pub mod as_of;
pub mod backfill;
pub mod channel;
pub mod command;
pub mod constant_price;
pub mod crud;
pub mod financial_freedom;
pub mod fund;
pub(crate) mod fx_week;
pub(crate) mod guards;
pub mod holdings;
pub mod ledger_tab;
pub mod lots;
pub mod manual_price;
pub mod market;
pub mod mwr;
pub mod overview;
pub mod predicates;
pub mod prices;
pub mod quote;
pub mod reports;
pub mod source;
pub mod split;
pub mod staleness;
pub mod stock;
pub mod trade;
pub mod transaction_seam;
pub mod trend;
pub mod unwind;

mod model;

pub use model::{
    AccountPnl, AddFundResult, AddStockInstrumentResult, CumulativePnlNativeTotal,
    CurrencyCumulativePnl, CurrencyHoldingTotals, FinancialFreedomOverview, Holding, Instrument,
    InstrumentInput, InstrumentListFilter, InstrumentListResult, InstrumentSourceDisplay,
    InstrumentType, InvestmentConvertFields, InvestmentOverview, InvestmentSplitFields,
    InvestmentTradeFields, InvestmentTransactionListFilter, InvestmentTransactionListResult,
    InvestmentTransactionRow, MANUAL_SOURCE, ManualPriceInput, ManualPriceResult, MarketPrice,
    MarketPriceInput, PnlFilter, PortfolioTrendPoint, PortfolioValueTrend, RealizedPnlSummary,
    TransactionConvert, TransactionSplit, TransactionTrade, TrendRange, YearPnl,
};

pub use backfill::{TrendBackfillState, TrendBackfillStatus};
pub use channel::{PriceChannel, derive_price_channel, derive_quote_market};
pub use command::{ExchangeRateCommand, InstrumentCommand, PriceCommand};
pub use command::{replay_exchange_rate_command, replay_instrument_command, replay_price_command};
pub use constant_price::{
    ConstantPriceValue, ensure_constant_base_price, mark_constant_unit_price,
};
pub use crud::{
    create_exchange_rate, create_instrument, create_instrument_manual, create_market_price,
    delete_instrument, list_exchange_rates, list_holdings, list_instruments, list_market_prices,
    upsert_auto_exchange_rate,
};
pub use financial_freedom::{query_financial_freedom, query_investable_assets_cents};
pub use fund::{
    FundCreateOutcome, add_fund_by_code_with, adopt_fund_quote, create_fund_degraded,
    is_six_digit_code, reject_carried_fund_market, validate_fund_code,
};
pub use ledger_tab::list_investment_transactions;
pub use manual_price::record_manual_price;
pub use market::{Market, QuoteMarket, StockRoute, derive_quote_currency};
pub use mwr::{
    AccountMwr, CurrencyMwr, InstrumentMwr, MoneyWeightedReturnSummary, MwrBasis, MwrRange,
    query_money_weighted_return_summary,
};
pub use overview::query_investment_overview;
pub use quote::{Quote, QuoteAdoptionInput, QuoteAdoptionOutcome};
pub use reports::{
    query_cumulative_pnl_native_total, query_cumulative_pnl_summary,
    query_holdings_summary_by_currency, query_realized_pnl_summary,
};
pub use source::source_display_by_transaction_ids;
pub use staleness::{PRICE_STALE_AFTER_DAYS, PriceStaleness, instrument_price_staleness};
pub use stock::{
    ResolvedStockCode, StockCreateOutcome, StockCreateRoute, StockEnhancePlan,
    add_stock_instrument_with_quote, adopt_stock_quote, create_stock_degraded,
    explicit_quote_market, fetch_stock_quote_for_add, resolve_add_stock_channel,
    resolve_stock_code, route_stock_creation,
};
pub use trade::{
    Plan, apply, convert_fields_by_transaction_ids, get_transaction_convert, get_transaction_split,
    get_transaction_trade, prepare, release_for_delete, revert,
};
pub use transaction_seam::install_transaction_hooks;
pub use trend::query_portfolio_value_trend;

#[cfg(test)]
mod tests;
