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
  /** 信用卡档案字段（spec #1327 / ADR-0119）：仅 `credit` 账户可携带，`null`/缺省 = 未设置。
   * **档案字段**：不参与余额与净资产口径（「还欠多少」由 `AccountBalance.balance_cents` 回答）。 */
  credit_limit_cents?: number | null;
  /** 账单日：1–31 的「每月第 N 日」声明值（额度/账单日/还款日三者彼此独立可空）。 */
  statement_day?: number | null;
  /** 还款日：1–31 的「每月第 N 日」声明值，与账单日不强制先后（银行存在跨月形态）。 */
  due_day?: number | null;
}

export interface AccountInput {
  name: string;
  type: AccountType;
  currency_code: string;
  initial_balance_cents?: number;
  /** 信用卡档案字段（仅信用卡账户可携带；缺省 = 未设置，事后可补填）。 */
  credit_limit_cents?: number;
  statement_day?: number;
  due_day?: number;
}

/** 账户编辑入参：type 不可改（参与余额符号归属）；币种仅无交易账户可改（后端拒绝）。
 * 信用卡档案字段三态：**缺省 = 不改、`null` = 清空、给值 = 落定该值**。 */
export interface AccountUpdateInput {
  name?: string;
  currency_code?: string;
  credit_limit_cents?: number | null;
  statement_day?: number | null;
  due_day?: number | null;
}

/** 余额调整入参：校准到目标值，后端生成一笔与黑洞账户的转账（ADR-0026）。 */
export interface AccountBalanceAdjustInput {
  target_balance_cents: number;
  date: string;
  note?: string;
}

export interface AccountBalance {
  account: Account;
  balance_cents: number;
}

/** 余额缓存审计差异行（issue #491）：缓存缺失记 null（回填前）。 */
export interface BalanceCacheDrift {
  account_id: string;
  account_name: string;
  cached_cents: number | null;
  actual_cents: number;
}

/** 余额缓存审计报告（issue #491）：修复已完成后的差异快照。 */
export interface BalanceCacheAudit {
  accounts_checked: number;
  drifts: BalanceCacheDrift[];
  repaired: boolean;
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
} satisfies Record<AccountType, boolean>;

export const ACCOUNT_TYPES = Object.keys(ACCOUNT_TYPE_PRESENCE) as AccountType[];

/** 出资账户准入闭集（issue #936 / ADR-0096 决策 4）：现金类账户 cash/bank/credit/ewallet/other。
 * 排除 investment（跨投资账户走 transfer，保住「子弹」语义单一）与 receivable/debt（借贷
 * 账户不承载投资结算）；与后端 transaction/funding.rs 的 FUNDING_ALLOWED_TYPES 同一闭集，
 * 后端行为层准入是唯一权威，本表驱动谓词仅供表单候选预过滤。币种一致过滤在表单层
 * （随交易币种），不在此闭集内。 */
const FUNDING_ACCOUNT_TYPE_PRESENCE = {
  cash: true,
  bank: true,
  credit: true,
  ewallet: true,
  other: true,
} satisfies Partial<Record<AccountType, boolean>>;

/** 出资账户候选谓词：账户类型在准入闭集内（币种一致过滤由调用方按交易币种承担） */
export function isFundingCandidateAccount(type: AccountType): boolean {
  return (FUNDING_ACCOUNT_TYPE_PRESENCE as Record<AccountType, boolean | undefined>)[type] === true;
}
