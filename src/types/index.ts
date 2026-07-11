export interface Currency {
  code: string
  name: string
  symbol: string
  decimal_places: number
}

export type AccountType = 'cash' | 'bank' | 'credit' | 'ewallet' | 'investment' | 'debt' | 'receivable' | 'other'

export interface Syncable {
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}

export interface Account extends Syncable {
  id: string
  name: string
  type: AccountType
  currency_code: string
  initial_balance_cents: number
  created_at: string
}

export interface AccountInput {
  name: string
  type: AccountType
  currency_code: string
  initial_balance_cents?: number
}

export interface AccountBalance {
  account: Account
  balance_cents: number
}

export type CategoryKind = 'income' | 'expense'

export interface Category extends Syncable {
  id: string
  name: string
  kind: CategoryKind
  parent_id: string | null
  icon: string | null
  color: string | null
  created_at: string
}

export interface CategoryInput {
  name: string
  kind: CategoryKind
  parent_id?: string | null
}

export type TransactionKind = 'income' | 'expense' | 'transfer' | 'refund' | 'buy' | 'sell'

export interface Transaction extends Syncable {
  id: string
  kind: TransactionKind
  amount_cents: number
  currency_code: string
  amount_native_cents: number
  account_id: string
  to_account_id: string | null
  category_id: string | null
  refund_of_transaction_id: string | null
  note: string | null
  date: string
  created_at: string
}

export interface TransactionInput {
  kind: TransactionKind
  amount_cents: number
  currency_code: string
  account_id: string
  to_account_id?: string | null
  category_id?: string | null
  refund_of_transaction_id?: string | null
  note?: string | null
  date: string
  instrument_id?: string | null
  quantity?: number | null
  price_cents?: number | null
  fee_cents?: number | null
}

export type BudgetPeriod = 'monthly' | 'yearly'

export const BUDGET_PERIOD_LABELS: Record<BudgetPeriod, string> = {
  monthly: '按月',
  yearly: '按年',
}

export interface Budget extends Syncable {
  id: string
  category_id: string
  period: BudgetPeriod
  amount_cents: number
  start_date: string
  created_at: string
}

export interface BudgetInput {
  category_id: string
  period?: BudgetPeriod
  amount_cents: number
  start_date: string
}

export interface BudgetProgress {
  budget: Budget
  category_name: string
  spent_cents: number
  over_budget: boolean
}

export interface MonthlySummary {
  month: string
  income_cents: number
  expense_cents: number
  refund_cents: number
}

export interface CategoryShare {
  category_id: string
  category_name: string
  amount_cents: number
}

export interface CreateTransactionResult {
  success: boolean
  id: string | null
  error: string | null
}

export interface ImportedRow {
  date: string
  amount_cents: number
  note: string
  category_name: string | null
}

export type InstrumentType = 'stock' | 'fund' | 'bond' | 'etf' | 'other'

export const INSTRUMENT_TYPE_LABELS: Record<InstrumentType, string> = {
  stock: '股票',
  fund: '基金',
  bond: '债券',
  etf: 'ETF',
  other: '其他',
}

export interface Instrument extends Syncable {
  id: string
  symbol: string
  type: InstrumentType
  name: string | null
  currency_code: string
  created_at: string
}

export interface InstrumentInput {
  symbol: string
  type: InstrumentType
  name?: string | null
  currency_code: string
}

/** 分 -> 元字符串，按币种小数位格式化 */
export function formatAmount(cents: number, currency?: Currency): string {
  const dp = currency?.decimal_places ?? 2
  const sign = cents < 0 ? '-' : ''
  const abs = Math.abs(cents)
  const value = abs / Math.pow(10, dp)
  const fixed = value.toFixed(dp)
  const symbol = currency?.symbol ?? ''
  return `${sign}${symbol}${fixed}`
}

export const ACCOUNT_TYPE_LABELS: Record<AccountType, string> = {
  cash: '现金',
  bank: '银行卡',
  credit: '信用卡',
  ewallet: '电子钱包',
  investment: '投资账户',
  debt: '负债',
  receivable: '借出款',
  other: '其他',
}

export const TRANSACTION_KIND_LABELS: Record<TransactionKind, string> = {
  income: '收入',
  expense: '支出',
  transfer: '转账',
  refund: '退款',
  buy: '买入',
  sell: '卖出',
}

export type ScheduledKind = 'installment' | 'subscription' | 'scheduled_transfer'
export type ScheduledStatus = 'active' | 'paused' | 'cancelled' | 'completed'
export type RecurrenceType = 'daily' | 'weekly' | 'monthly' | 'yearly'

export interface ScheduledTransaction {
  id: string
  kind: ScheduledKind
  status: ScheduledStatus
  account_id: string
  category_id: string | null
  amount_cents: number
  currency_code: string
  recurrence_type: RecurrenceType
  recurrence_interval: number
  recurrence_day: number | null
  start_date: string
  note: string | null
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}

export interface ScheduledTransactionOccurrence {
  id: string
  scheduled_transaction_id: string
  scheduled_date: string
  status: 'pending' | 'processing' | 'completed' | 'failed' | 'cancelled'
  transaction_id: string | null
  amount_cents: number
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}

export interface ScheduledTransactionWithExt {
  core: ScheduledTransaction
  counterparty: string | null
  total_amount_cents: number | null
  total_occurrences: number | null
  to_account_id: string | null
}

export interface ScheduledTransactionDetail {
  core: ScheduledTransaction
  extension: InstallmentPlan | SubscriptionPlan | ScheduledTransferPlan
  pending_occurrences: ScheduledTransactionOccurrence[]
  completed_occurrences: number
}

export interface InstallmentPlan {
  scheduled_transaction_id: string
  counterparty: string | null
  total_amount_cents: number
  total_occurrences: number
}

export interface SubscriptionPlan {
  scheduled_transaction_id: string
  counterparty: string | null
}

export interface ScheduledTransferPlan {
  scheduled_transaction_id: string
  to_account_id: string
  total_occurrences: number | null
}

export interface CreateScheduledInput {
  kind: ScheduledKind
  account_id: string
  category_id?: string | null
  amount_cents: number
  currency_code: string
  recurrence_type: RecurrenceType
  recurrence_interval: number
  recurrence_day?: number | null
  start_date: string
  note?: string | null
  counterparty?: string | null
  total_amount_cents?: number | null
  total_occurrences?: number | null
  to_account_id?: string | null
}

export interface UpdateStatusInput {
  id: string
  new_status: ScheduledStatus
}

export interface ExecuteOccurrenceInput {
  occurrence_id: string
}

export interface ExchangeRate {
  id: string
  base_code: string
  quote_code: string
  rate: number
  priced_at: string
  source: string | null
  updated_at: string
  version: number
  device_id: string
}

export interface ExchangeRateInput {
  base_code: string
  quote_code: string
  rate: number
  priced_at: string
  source?: string | null
}

export interface MarketPrice {
  id: string
  instrument_id: string
  price_cents: number
  currency_code: string
  priced_at: string
  source: string | null
  created_at: string
  updated_at: string
  version: number
  device_id: string
}

export interface MarketPriceInput {
  instrument_id: string
  price_cents: number
  currency_code: string
  priced_at: string
  source?: string | null
}

export interface Holding {
  id: string
  account_id: string
  instrument_id: string
  quantity: number
  cost_basis_cents: number
  cost_currency_code: string
  latest_price_cents: number | null
  latest_price_currency_code: string | null
  market_value_cents: number | null
  unrealized_pnl_cents: number | null
  updated_at: string
}
