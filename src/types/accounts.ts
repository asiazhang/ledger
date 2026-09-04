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

/** 余额缓存审计差异行（issue #491）：缓存缺失记 null（回填前）。 */
export interface BalanceCacheDrift {
  account_id: string
  account_name: string
  cached_cents: number | null
  actual_cents: number
}

/** 余额缓存审计报告（issue #491）：修复已完成后的差异快照。 */
export interface BalanceCacheAudit {
  accounts_checked: number
  drifts: BalanceCacheDrift[]
  repaired: boolean
}

/** 账户类型闭集（穷尽表驱动：新增 AccountType 变体未列出即编译报错）；
 * 显示标签在文案资源 accounts.type.*（i18n，ADR-0049），不再硬编码。 */
const ACCOUNT_TYPE_PRESENCE = {
  cash: true,
  bank: true,
  credit: true,
  ewallet: true,
  investment: true,
  debt: true,
  receivable: true,
  other: true,
} satisfies Record<AccountType, boolean>

export const ACCOUNT_TYPES = Object.keys(ACCOUNT_TYPE_PRESENCE) as AccountType[]
