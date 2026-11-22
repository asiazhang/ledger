import type { Syncable } from "./common";

export type AccountType =
  | "cash"
  | "bank"
  | "credit"
  | "ewallet"
  | "investment"
  | "debt"
  | "receivable"
  | "other";

export interface Account extends Syncable {
  id: string;
  name: string;
  type: AccountType;
  currency_code: string;
  initial_balance_cents: number;
  created_at: string;
  is_hidden: boolean;
  is_archived: boolean;
  credit_limit_cents?: number | null;
  statement_day?: number | null;
  due_day?: number | null;
}

export interface AccountInput {
  name: string;
  type: AccountType;
  currency_code: string;
  initial_balance_cents?: number;
  credit_limit_cents?: number;
  statement_day?: number;
  due_day?: number;
}

export interface AccountUpdateInput {
  name?: string;
  currency_code?: string;
  credit_limit_cents?: number | null;
  statement_day?: number | null;
  due_day?: number | null;
  is_archived?: boolean | null;
}

export interface AccountBalanceAdjustInput {
  target_balance_cents: number;
  date: string;
  note?: string;
}

export interface AccountBalance {
  account: Account;
  balance_cents: number;
}

export interface BalanceCacheDrift {
  account_id: string;
  account_name: string;
  cached_cents: number | null;
  actual_cents: number;
}

export interface BalanceCacheAudit {
  accounts_checked: number;
  drifts: BalanceCacheDrift[];
  repaired: boolean;
}

const ACCOUNT_TYPE_PRESENCE = {
  cash: true,
  bank: true,
  credit: true,
  ewallet: true,
  investment: true,
  debt: true,
  receivable: true,
  other: true,
} satisfies Record<AccountType, boolean>;

export const ACCOUNT_TYPES = Object.keys(ACCOUNT_TYPE_PRESENCE) as AccountType[];

const FUNDING_ACCOUNT_TYPE_PRESENCE = {
  cash: true,
  bank: true,
  credit: true,
  ewallet: true,
  other: true,
} satisfies Partial<Record<AccountType, boolean>>;

export function isFundingCandidateAccount(type: AccountType): boolean {
  return (FUNDING_ACCOUNT_TYPE_PRESENCE as Record<AccountType, boolean | undefined>)[type] === true;
}
