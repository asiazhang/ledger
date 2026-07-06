export interface Currency {
  code: string
  name: string
  symbol: string
  decimal_places: number
}

export type AccountType = 'cash' | 'bank' | 'credit' | 'savings' | 'ewallet' | 'debt' | 'receivable'

export interface Account {
  id: number
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

export interface Category {
  id: number
  name: string
  kind: CategoryKind
  parent_id: number | null
  icon: string | null
  color: string | null
  created_at: string
}

export interface CategoryInput {
  name: string
  kind: CategoryKind
  parent_id?: number | null
}

export type TransactionKind = 'income' | 'expense' | 'transfer' | 'refund'

export interface Transaction {
  id: number
  kind: TransactionKind
  amount_cents: number
  currency_code: string
  amount_native_cents: number
  account_id: number
  to_account_id: number | null
  category_id: number | null
  refund_of_transaction_id: number | null
  note: string | null
  date: string
  created_at: string
}

export interface TransactionInput {
  kind: TransactionKind
  amount_cents: number
  currency_code: string
  account_id: number
  to_account_id?: number | null
  category_id?: number | null
  refund_of_transaction_id?: number | null
  note?: string | null
  date: string
}

export interface Budget {
  id: number
  category_id: number
  period: string
  amount_cents: number
  start_date: string
}

export interface BudgetInput {
  category_id: number
  period?: string
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
  category_id: number
  category_name: string
  amount_cents: number
}

export interface ImportedRow {
  date: string
  amount_cents: number
  note: string
  category_name: string | null
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
  savings: '储蓄',
  ewallet: '电子钱包',
  debt: '负债',
  receivable: '债权',
}

export const TRANSACTION_KIND_LABELS: Record<TransactionKind, string> = {
  income: '收入',
  expense: '支出',
  transfer: '转账',
  refund: '退款',
}
