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
