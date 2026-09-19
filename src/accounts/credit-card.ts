import type { Account } from "@ledger/types";
import { formatLocalDateISO } from "@ledger/utils/date";

/**
 * 信用卡档案派生（spec #1327 / ADR-0119）：额度用量与「下次账单日 / 下次还款日」。
 *
 * 本模块是全仓唯一的信用卡派生点：账户列表的使用率小字与编辑弹窗的只读摘要
 * 共用 [`creditProfile`]，不出现第二份算法。纯函数、无 Vue/store 依赖（与
 * `domain/lending.ts` 同一纪律），也不消费 `t()`——文案留给视图侧。
 *
 * 派生量**不落库、不参与余额口径**：已用额度由余额派生（`max(0, −余额)`，余额为负
 * 即欠款）、可用额度 = 额度 + 溢缴款（`max(0, 余额)`），与 `account_flow` 符号矩阵
 * 同源；后端只存三个档案字段（额度 / 账单日 / 还款日）。
 */

/** 使用率警示阈值（%）：达到或超过它时列表小字用警示色。阈值只此一处，不做设置项。 */
export const CREDIT_UTILIZATION_WARNING_PERCENT = 90;

/** 信用卡档案字段（`null` = 未设置）。 */
export interface CreditTerms {
  creditLimitCents: number | null;
  statementDay: number | null;
  dueDay: number | null;
}

/** 信用卡派生视图（账户列表与编辑弹窗共用的唯一形态）。 */
export interface CreditProfile {
  /** 信用额度（未设置 = `null`）。 */
  limitCents: number | null;
  /** 已用额度 = `max(0, −余额)`（余额为正即溢缴款，此时已用为 0）。 */
  usedCents: number;
  /** 可用额度 = 额度 − 已用 + 溢缴款（= 额度 + 余额）；额度未设置时为 `null`。
   * 超额使用时为负值（如实呈现而不是钳到 0：與使用率 >100% 同口径）。 */
  availableCents: number | null;
  /** 使用率 %（四舍五入取整）；额度未设置时为 `null`。 */
  utilizationPercent: number | null;
  /** 下次账单日（ISO 短格式）；账单日未设置时为 `null`。 */
  nextStatementDate: string | null;
  /** 下次还款日（ISO 短格式）；还款日未设置时为 `null`。 */
  nextDueDate: string | null;
}

/** 账户上参与信用卡派生的字段（结构化窄口，便于视图与测试构造）。 */
export type CreditAccountFields = Pick<
  Account,
  "type" | "credit_limit_cents" | "statement_day" | "due_day"
>;

/** 已用额度：余额为负即欠款，取其绝对值为已用；余额 ≥ 0（无欠款/溢缴款）时为 0。 */
export function usedCreditCents(balanceCents: number): number {
  return Math.max(0, -balanceCents);
}

/**
 * 可用额度 = 信用额度 − 已用额度 + 溢缴款；三者同时化简为 **额度 + 余额**
 * （余额 = 溢缴款 − 已用，已由 `account_flow` 符号矩阵给出，不另造第二口径），
 * 即银行 App 的「可用额度」。额度未设置返回 `null`；超额使用时为负值（如实呈现，
 * 不钳到 0——与使用率 >100% 同口径）。
 */
export function availableCreditCents(
  balanceCents: number,
  limitCents: number | null | undefined,
): number | null {
  if (limitCents == null || limitCents <= 0) return null;
  return limitCents + balanceCents;
}

/** 使用率 %（整数、四舍五入）；额度未设置返回 `null`（0 不算额度，是非法值）。 */
export function creditUtilizationPercent(
  balanceCents: number,
  limitCents: number | null | undefined,
): number | null {
  if (limitCents == null || limitCents <= 0) return null;
  return Math.round((usedCreditCents(balanceCents) / limitCents) * 100);
}

/** 该日是否是可用的「每月第 N 日」（1–31 整数，其余视作未设置/脏值）。 */
function isValidDay(day: number | null | undefined): day is number {
  return day != null && Number.isInteger(day) && day >= 1 && day <= 31;
}

/**
 * 「下次某日」：从 `today`（本地日历日）起最近的该日，按本地日历日语义。
 *
 * - 当月没有该日时取当月最后一天（31 日 → 2 月取 28/29 日），与后端调度器的
 *   月末钳制同口径（展示层钳制，不落库）；
 * - 今天就是该日时取今天（不跳到下个月）；
 * - 日越界或未设置时返回 `null`（不猜、不截断成 1 日）。
 */
export function nextOccurrenceOfDay(day: number | null | undefined, today: Date): string | null {
  if (!isValidDay(day)) return null;
  const year = today.getFullYear();
  const month0 = today.getMonth();
  const clamped = Math.min(day, daysInMonth(year, month0));
  if (today.getDate() <= clamped) return formatLocalDateISO(year, month0, clamped);
  const nextYear = month0 === 11 ? year + 1 : year;
  const nextMonth0 = (month0 + 1) % 12;
  return formatLocalDateISO(nextYear, nextMonth0, Math.min(day, daysInMonth(nextYear, nextMonth0)));
}

/** 本地「第 m0 月（0 起）」的天数：经「次月 0 日」滚动得出（自动处理闰年）。 */
function daysInMonth(year: number, month0: number): number {
  return new Date(year, month0 + 1, 0).getDate();
}

/**
 * 信用卡派生视图（唯一入口）：非信用卡账户返回 `null`——调用方据此分支，不散落
 * `type === 'credit'` 判断；`today` 可注入（测试用），缺省取当前本地日期。
 */
export function creditProfile(
  account: CreditAccountFields,
  balanceCents: number,
  today: Date = new Date(),
): CreditProfile | null {
  if (account.type !== "credit") return null;
  const limitCents = account.credit_limit_cents ?? null;
  const statementDay = account.statement_day ?? null;
  const dueDay = account.due_day ?? null;
  return {
    limitCents,
    usedCents: usedCreditCents(balanceCents),
    availableCents: availableCreditCents(balanceCents, limitCents),
    utilizationPercent: creditUtilizationPercent(balanceCents, limitCents),
    nextStatementDate: nextOccurrenceOfDay(statementDay, today),
    nextDueDate: nextOccurrenceOfDay(dueDay, today),
  };
}

/**
 * 账户列表的使用率小字判据：**仅在已用 > 0 且额度已设置**时返回百分比，其余
 * （无欠款 / 溢缴款 / 未设置额度）返回 `null` —— 0% 无警示价值、只增噪音，
 * 稀疏显示反过来说强化「这张卡额度吃紧了」的可扫性。
 */
export function listUtilizationPercent(
  account: CreditAccountFields,
  balanceCents: number,
): number | null {
  const profile = creditProfile(account, balanceCents);
  if (profile === null || profile.usedCents <= 0) return null;
  return profile.utilizationPercent;
}
