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

/** 账户编辑入参：type 不可改（参与余额符号归属）；币种仅无交易账户可改（后端拒绝）。 */
export interface AccountUpdateInput {
  name?: string
  currency_code?: string
}

/** 余额调整入参：校准到目标值，后端生成一笔与黑洞账户的转账（ADR-0026）。 */
export interface AccountBalanceAdjustInput {
  target_balance_cents: number
  date: string
  note?: string
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
