import type { Syncable } from "./common";
import type { TransactionKind } from "./transactions";

export type InstrumentType = "stock" | "fund" | "bond" | "etf" | "other";

export type MarketType = "sh" | "sz" | "hk" | "nasdaq" | "nyse" | "amex" | "unknown";

export const MARKET_TYPES: MarketType[] = ["sh", "sz", "hk", "nasdaq", "nyse", "amex", "unknown"];

export const MARKET_FILTER_TYPES: MarketType[] = ["sh", "sz", "hk", "unknown"];

export type AddInstrumentChannel = "sh" | "sz" | "hk" | "us" | "fund" | "custom";

export const INSTRUMENT_TYPES: InstrumentType[] = ["stock", "fund", "bond", "etf", "other"];

export type InstrumentSource = "eastmoney" | "manual";

export interface Instrument extends Syncable {
  id: string;
  symbol: string;
  type: InstrumentType;
  name: string | null;
  currency_code: string;
  market: MarketType;
  created_at: string;
  source: InstrumentSource;
  price_cents: number | null;
  invested: boolean;
  price_channel: InstrumentPriceChannel;
}

export type InstrumentPriceChannel = "quote" | "fund_nav" | "constant" | "manual" | "none";

export const INSTRUMENT_PRICE_CHANNELS: InstrumentPriceChannel[] = [
  "quote",
  "fund_nav",
  "constant",
  "manual",
  "none",
];

export interface PriceStaleness {
  stale_count: number;
  threshold_days: number;
}

export interface InvestmentOverview {
  native_currency: string;
  investable_assets_cents: number;
  investment_cash_cents: number;
  holdings_market_value_cents: number;
  total_market_value_cents: number;
  unrealized_pnl_cents: number;
  cumulative_pnl_cents: number;
  missing_price_holding_count: number;
  has_investment_account: boolean;
}

export interface InstrumentInput {
  symbol: string;
  type: InstrumentType;
  name?: string | null;
  currency_code: string;
  market?: MarketType | null;
}

export interface InstrumentListFilter {
  search?: string | null;
  market?: MarketType | null;
  type?: InstrumentType | null;
  only_invested?: boolean | null;
  page?: number;
  page_size?: number;
  anchor_id?: string | null;
}

export interface InstrumentListResult {
  items: Instrument[];
  total: number;
  anchor_page?: number;
}

export interface ManualPriceInput {
  instrument_id: string;
  date: string;
  price_cents: number;
}

export interface ManualPriceResult {
  history_written: boolean;
  current_price_written: boolean;
}

export interface AddFundResult {
  instrument_id: string;
  symbol: string;
  name: string;
  fund_class: string;
  nav_cents: number | null;
  nav_date: string | null;
  price_written: boolean;
}

export interface AddStockInstrumentResult {
  instrument_id: string;
  symbol: string;
  name: string;
  type: InstrumentType;
  market: MarketType;
  currency_code: string;
  price_cents: number | null;
  price_date: string | null;
  price_written: boolean;
}

export interface TransactionTrade {
  instrument_id: string;
  symbol: string;
  instrument_name: string | null;
  instrument_type: InstrumentType;
  quantity: number;
  price_cents: number;
  fee_cents: number | null;
}

export interface TransactionConvert {
  out_instrument_id: string;
  out_symbol: string;
  out_instrument_name: string | null;
  out_quantity: number;
  out_amount_cents: number;
  in_instrument_id: string;
  in_symbol: string;
  in_instrument_name: string | null;
  in_quantity: number;
  in_amount_cents: number;
  fee_cents: number;
  carried_cost_cents: number;
  currency_code: string;
}

export interface TransactionSplit {
  instrument_id: string;
  symbol: string;
  instrument_name: string | null;
  quantity: number;
}

export interface InvestmentTradeFields {
  quantity: number;
  price_cents: number;
  fee_cents: number;
}

export interface InvestmentConvertFields {
  quantity: number;
  to_instrument_id: string;
  to_symbol: string;
  to_quantity: number;
  out_amount_cents: number;
  in_amount_cents: number;
}

export interface InvestmentSplitFields {
  delta_quantity: number;
}

export interface InvestmentTransactionRow {
  id: string;
  date: string;
  kind: TransactionKind;
  amount_cents: number;
  account_id: string;
  funding_account_id: string | null;
  instrument_id: string;
  symbol: string;
  instrument_name: string | null;
  instrument_type: InstrumentType;
  trade: InvestmentTradeFields | null;
  convert: InvestmentConvertFields | null;
  split: InvestmentSplitFields | null;
  note: string | null;
  currency_code: string;
  amount_native_cents: number;
  source_order_no: string | null;
}

export interface InvestmentTransactionListFilter {
  account_id?: string | null;
  instrument_id?: string | null;
  kinds?: TransactionKind[] | null;
  from?: string | null;
  to?: string | null;
  page?: number;
  page_size?: number;
}

export interface InvestmentTransactionListResult {
  items: InvestmentTransactionRow[];
  total: number;
}

export interface Holding {
  id: string;
  account_id: string;
  instrument_id: string;
  quantity: number;
  cost_basis_cents: number;
  cost_currency_code: string;
  latest_price_cents: number | null;
  latest_price_currency_code: string | null;
  latest_nav_date: string | null;
  market_value_cents: number | null;
  unrealized_pnl_cents: number | null;
  native_market_value_cents: number | null;
  native_unrealized_pnl_cents: number | null;
  updated_at: string;
}

export interface MarketPrice {
  id: string;
  instrument_id: string;
  price_cents: number;
  currency_code: string;
  priced_at: string;
  source: string | null;
  created_at: string;
  updated_at: string;
  version: number;
  device_id: string;
}

export interface MarketPriceInput {
  instrument_id: string;
  price_cents: number;
  currency_code: string;
  priced_at: string;
  source?: string | null;
}

export interface YearPnl {
  year: string;
  native_currency: string;
  realized_pnl_cents: number | null;
  dividend_cents: number | null;
  realized_gain_cents: number | null;
}

export interface AccountPnl {
  account_id: string;
  account_name: string;
  native_currency: string;
  realized_pnl_cents: number | null;
  dividend_cents: number | null;
  realized_gain_cents: number | null;
}

export interface PnlFilter {
  account_id?: string | null;
  instrument_id?: string | null;
}

export interface CurrencyCumulativePnl {
  currency_code: string;
  cumulative_pnl_cents: number;
}

export interface CumulativePnlNativeTotal {
  total_cents: number;
  native_currency: string;
}

export interface RealizedPnlSummary {
  by_year: YearPnl[];
  by_account: AccountPnl[];
}

export interface TrendRange {
  start_date?: string | null;
  end_date?: string | null;
}

export type TrendBackfillState = "running" | "retry_pending" | "no_data";

export interface TrendBackfillStatus {
  state: TrendBackfillState;
  done?: number;
  total?: number;
}

export interface PortfolioTrendPoint {
  date: string;
  market_value_cents: number;
}

export interface PortfolioValueTrend {
  currency_code: string;
  points: PortfolioTrendPoint[];
  backfill?: TrendBackfillStatus;
}

export interface MwrRange {
  start_date?: string | null;
  end_date?: string | null;
}

export type MwrBasis = "annualized" | "cumulative";

export interface InstrumentMwr {
  account_id: string;
  instrument_id: string;
  currency_code: string;
  basis: MwrBasis;
  rate: number | null;
}

export interface AccountMwr {
  account_id: string;
  account_name: string;
  currency_code: string;
  basis: MwrBasis;
  rate: number | null;
}

export interface CurrencyMwr {
  currency_code: string;
  basis: MwrBasis;
  rate: number | null;
}

export interface MoneyWeightedReturnSummary {
  by_instrument: InstrumentMwr[];
  by_account: AccountMwr[];
  total: CurrencyMwr[];
}
