import type {
  CreateTransactionResult,
  OrderAccountContribution,
  Transaction,
  TransactionInput,
  TransactionOrderSummary,
  TransactionSearchResult,
} from "@ledger/types";
import { insertTransaction } from "./ledger";
import { argsOf, db, emitLedgerChanged, obj, str } from "./state";

const MAX_PAGE_SIZE = 200;

function optStr(args: Record<string, unknown>, key: string): string | null {
  const v = args[key];
  return typeof v === "string" ? v : null;
}

function optNum(args: Record<string, unknown>, key: string): number | null {
  const v = args[key];
  return typeof v === "number" ? v : null;
}

export function searchTransactions(args: Record<string, unknown>): TransactionSearchResult {
  const a = argsOf(args);
  const terms = (typeof a.query === "string" ? a.query : "")
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((term) => term.toLowerCase());
  const min = optNum(a, "amountMinCents");
  const max = optNum(a, "amountMaxCents");
  const from = optStr(a, "dateFrom");
  const to = optStr(a, "dateTo");
  if (terms.length === 0 && min === null && max === null && from === null && to === null) {
    return { items: [], total: 0 };
  }

  const merchantNames = new Map(db().merchants.map((m) => [m.id, m.name.toLowerCase()]));
  const rows = db()
    .transactions.filter((t) => {
      if (t.is_deleted) return false;
      if (min !== null && t.amount_native_cents < min) return false;
      if (max !== null && t.amount_native_cents > max) return false;
      if (from !== null && t.date < from) return false;
      if (to !== null && t.date > to) return false;
      for (const term of terms) {
        const inNote = t.note !== null && t.note.toLowerCase().includes(term);
        const inMerchant =
          t.merchant_id !== null && (merchantNames.get(t.merchant_id) ?? "").includes(term);
        if (!inNote && !inMerchant) return false;
      }
      return true;
    })
    .sort(
      (x, y) =>
        y.date.localeCompare(x.date) ||
        y.created_at.localeCompare(x.created_at) ||
        y.id.localeCompare(x.id),
    );

  const page = Math.max(1, optNum(a, "page") ?? 1);
  const pageSize = Math.min(MAX_PAGE_SIZE, Math.max(1, optNum(a, "pageSize") ?? 20));
  const start = (page - 1) * pageSize;
  return { items: rows.slice(start, start + pageSize), total: rows.length };
}

export function createTransactions(args: Record<string, unknown>): CreateTransactionResult[] {
  const inputs = obj<TransactionInput[]>(args, "inputs");
  if (!Array.isArray(inputs)) throw new Error("替身后端：命令参数 inputs 缺失");
  const results = inputs.map((input) => {
    const id = insertTransaction(input);
    return { success: true, duplicate: false, id, error: null };
  });
  emitLedgerChanged();
  return results;
}

export function getTransactionOrderSummary(args: Record<string, unknown>): TransactionOrderSummary {
  const sourceOrderNo = str(args, "sourceOrderNo");
  const items: Transaction[] = db()
    .transactions.filter((t) => !t.is_deleted && t.source_order_no === sourceOrderNo)
    .sort(
      (x, y) =>
        x.date.localeCompare(y.date) ||
        x.created_at.localeCompare(y.created_at) ||
        x.id.localeCompare(y.id),
    );

  const row_count = items.length;
  const total_amount_cents = items.reduce((sum, t) => sum + t.amount_cents, 0);
  const currency_code =
    row_count === 0 || items.some((t) => t.currency_code !== items[0]?.currency_code)
      ? null
      : (items[0]?.currency_code ?? null);

  const accounts: OrderAccountContribution[] = [];
  for (const t of items) {
    const contributions =
      t.fundings.length > 0
        ? t.fundings.map((f) => ({ account_id: f.account_id, amount_cents: f.amount_cents }))
        : t.account_id !== null
          ? [{ account_id: t.account_id, amount_cents: t.amount_cents }]
          : [];
    for (const c of contributions) {
      const hit = accounts.find((x) => x.account_id === c.account_id);
      if (hit) hit.amount_cents += c.amount_cents;
      else accounts.push({ ...c });
    }
  }

  return {
    source_order_no: sourceOrderNo,
    row_count,
    total_amount_cents,
    currency_code,
    accounts,
    items,
  };
}
