import type { Account } from "@ledger/types";
import { formatLocalDateISO } from "@ledger/utils/date";

export const CREDIT_UTILIZATION_WARNING_PERCENT = 90;

export interface CreditProfile {
  limitCents: number | null;
  usedCents: number;
  availableCents: number | null;
  utilizationPercent: number | null;
  nextStatementDate: string | null;
  nextDueDate: string | null;
}

export type CreditAccountFields = Pick<
  Account,
  "type" | "credit_limit_cents" | "statement_day" | "due_day"
>;

export function usedCreditCents(balanceCents: number): number {
  return Math.max(0, -balanceCents);
}

export function availableCreditCents(
  balanceCents: number,
  limitCents: number | null | undefined,
): number | null {
  if (limitCents == null || limitCents <= 0) return null;
  return limitCents + balanceCents;
}

export function creditUtilizationPercent(
  balanceCents: number,
  limitCents: number | null | undefined,
): number | null {
  if (limitCents == null || limitCents <= 0) return null;
  return Math.round((usedCreditCents(balanceCents) / limitCents) * 100);
}

function isValidDay(day: number | null | undefined): day is number {
  return day != null && Number.isInteger(day) && day >= 1 && day <= 31;
}

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

function daysInMonth(year: number, month0: number): number {
  return new Date(year, month0 + 1, 0).getDate();
}

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

export function listUtilizationPercent(
  account: CreditAccountFields,
  balanceCents: number,
): number | null {
  const profile = creditProfile(account, balanceCents);
  if (profile === null || profile.usedCents <= 0) return null;
  return profile.utilizationPercent;
}
