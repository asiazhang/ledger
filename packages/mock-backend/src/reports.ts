import type {
  CategoryShare,
  MerchantSharesReport,
  MonthlySummary,
  ReportDateRange,
  Transaction,
} from "@ledger/types";
import { argsOf, db, str } from "./state";

function activeTxns(): Transaction[] {
  return db().transactions.filter((t) => !t.is_deleted);
}

function optStr(args: Record<string, unknown>, key: string): string | null {
  const v = args[key];
  return typeof v === "string" ? v : null;
}

function optNum(args: Record<string, unknown>, key: string): number | null {
  const v = args[key];
  return typeof v === "number" ? v : null;
}

function periodPredicate(
  from: string | null,
  to: string | null,
): ((date: string) => boolean) | null {
  if (from === null && to === null) return null;
  return (date: string) => (from === null || date >= from) && (to === null || date <= to);
}

function txnFilter(
  period: ((date: string) => boolean) | null,
  legacyMonth: string | null,
  legacyYear: number | null,
): (t: Transaction) => boolean {
  return (t) => {
    if (period) return period(t.date);
    if (legacyMonth !== null && !t.date.startsWith(legacyMonth)) return false;
    if (legacyYear !== null && !t.date.startsWith(String(legacyYear))) return false;
    return true;
  };
}

function incomeNetCents(t: Transaction): number {
  return t.kind === "income" || t.kind === "dividend" ? t.amount_native_cents : 0;
}

function expenseGrossCents(t: Transaction): number {
  return t.kind === "expense" ? t.amount_native_cents : 0;
}

function refundGrossCents(t: Transaction): number {
  return t.kind === "refund" ? t.amount_native_cents : 0;
}

function expenseNetCents(t: Transaction): number {
  if (t.kind === "expense") return t.amount_native_cents;
  if (t.kind === "refund") return -t.amount_native_cents;
  return 0;
}

export function reportDateRange(): ReportDateRange {
  const dates = activeTxns().map((t) => t.date);
  if (dates.length === 0) return { min_date: null, max_date: null };
  const sorted = [...dates].sort();
  return { min_date: sorted[0] ?? null, max_date: sorted[sorted.length - 1] ?? null };
}

export function monthlySummary(args: Record<string, unknown>): MonthlySummary[] {
  const a = argsOf(args);
  const predicate = txnFilter(
    periodPredicate(optStr(a, "from"), optStr(a, "to")),
    null,
    optNum(a, "year"),
  );
  const byMonth = new Map<string, MonthlySummary>();
  for (const t of activeTxns()) {
    if (!predicate(t)) continue;
    const month = t.date.slice(0, 7);
    const row = byMonth.get(month) ?? { month, income_cents: 0, expense_cents: 0, refund_cents: 0 };
    row.income_cents += incomeNetCents(t);
    row.expense_cents += expenseGrossCents(t);
    row.refund_cents += refundGrossCents(t);
    byMonth.set(month, row);
  }
  return [...byMonth.values()].sort((x, y) => x.month.localeCompare(y.month));
}

export function categoryShares(args: Record<string, unknown>): CategoryShare[] {
  const kind = str(args, "kind");
  const a = argsOf(args);
  const predicate = txnFilter(
    periodPredicate(optStr(a, "from"), optStr(a, "to")),
    optStr(a, "month"),
    optNum(a, "year"),
  );
  const isExpense = kind === "expense";
  const byCategory = new Map<string | null, number>();
  for (const t of activeTxns()) {
    if (!predicate(t)) continue;
    const cents = isExpense ? expenseNetCents(t) : incomeNetCents(t);
    if (cents === 0) continue;
    byCategory.set(t.category_id, (byCategory.get(t.category_id) ?? 0) + cents);
  }
  const categories = db().categories;
  return [...byCategory.entries()]
    .map(([categoryId, amount_cents]) => ({
      category_id: categoryId ?? "",
      category_name: categories.find((c) => c.id === categoryId)?.name ?? "未分类",
      amount_cents,
    }))
    .sort((x, y) => y.amount_cents - x.amount_cents || x.category_id.localeCompare(y.category_id));
}

export function merchantShares(args: Record<string, unknown>): MerchantSharesReport {
  const a = argsOf(args);
  const predicate = txnFilter(
    periodPredicate(optStr(a, "from"), optStr(a, "to")),
    null,
    optNum(a, "year"),
  );
  const byMerchant = new Map<string, { net: number; count: number }>();
  for (const t of activeTxns()) {
    if (t.merchant_id === null || !predicate(t)) continue;
    const cents = expenseNetCents(t);
    if (cents === 0) continue;
    const acc = byMerchant.get(t.merchant_id) ?? { net: 0, count: 0 };
    acc.net += cents;
    acc.count += 1;
    byMerchant.set(t.merchant_id, acc);
  }
  const merchants = db().merchants;
  const rows = [...byMerchant.entries()]
    .map(([merchant_id, { net, count }]) => ({
      merchant_id,
      merchant_name: merchants.find((m) => m.id === merchant_id)?.name ?? "",
      amount_cents: net,
      transaction_count: count,
    }))
    .sort(
      (x, y) => y.amount_cents - x.amount_cents || (x.merchant_name < y.merchant_name ? -1 : 1),
    );
  const total_cents = rows.reduce((sum, r) => sum + r.amount_cents, 0);
  const topN = optNum(a, "topN");
  return { rows: topN !== null ? rows.slice(0, Math.max(0, topN)) : rows, total_cents };
}
