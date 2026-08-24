import type { Syncable } from './common'

export type AccountType = 'cash' | 'bank' | 'credit' | 'ewallet' | 'investment' | 'debt' | 'receivable' | 'other'

export interface Account extends Syncable {
  id: string
  name: string
  type: AccountType
  currency_code: string
  initial_balance_cents: number
  created_at: string
  is_hidden: boolean
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
