import type {
  InvestmentConvertFields,
  InvestmentTransactionListFilter,
  InvestmentTransactionListResult,
  InvestmentTransactionRow,
  InvestmentTradeFields,
  InvestmentSplitFields,
  Transaction,
  TransactionConvert,
  TransactionKind,
  TransactionSplit,
  TransactionTrade,
} from "@ledger/types";
import { argsOf, db, findActive, obj, str } from "./state";
import type { MockSecurityTxn } from "./state";

const INVESTMENT_KINDS: ReadonlySet<TransactionKind> = new Set([
  "buy",
  "sell",
  "convert",
  "split",
  "dividend",
]);

function securityTxnOf(transactionId: string): MockSecurityTxn | undefined {
  return db().securityTransactions.find((s) => s.transaction_id === transactionId);
}

function instrumentOf(id: string) {
  return db().instruments.find((i) => i.id === id);
}

function projectRow(t: Transaction, st: MockSecurityTxn): InvestmentTransactionRow {
  const instrument = instrumentOf(st.instrument_id);
  const symbol = instrument?.symbol ?? "";
  const instrumentName = instrument?.name ?? null;
  const instrumentType = instrument?.type ?? "other";

  let trade: InvestmentTradeFields | null = null;
  let convert: InvestmentConvertFields | null = null;
  let split: InvestmentSplitFields | null = null;
  if (t.kind === "buy" || t.kind === "sell") {
    trade = {
      quantity: st.quantity ?? 0,
      price_cents: st.price_cents ?? 0,
      fee_cents: st.fee_cents,
    };
  } else if (t.kind === "convert" && st.to_instrument_id) {
    convert = {
      quantity: st.quantity ?? 0,
      to_instrument_id: st.to_instrument_id,
      to_symbol: instrumentOf(st.to_instrument_id)?.symbol ?? "",
      to_quantity: st.to_quantity ?? 0,
      out_amount_cents: st.out_amount_cents ?? 0,
      in_amount_cents: st.in_amount_cents ?? 0,
    };
  } else if (t.kind === "split") {
    split = { delta_quantity: st.quantity ?? 0 };
  }

  return {
    id: t.id,
    date: t.date,
    kind: t.kind,
    amount_cents: t.amount_cents,
    account_id: t.account_id ?? "",
    funding_account_id: t.funding_account_id,
    instrument_id: st.instrument_id,
    symbol,
    instrument_name: instrumentName,
    instrument_type: instrumentType,
    trade,
    convert,
    split,
    note: t.note,
    currency_code: t.currency_code,
    amount_native_cents: t.amount_native_cents,
    source_order_no: t.source_order_no,
  };
}

export function listInvestmentTransactions(
  args: Record<string, unknown>,
): InvestmentTransactionListResult {
  const filter = (obj<InvestmentTransactionListFilter | null>(argsOf(args), "filter") ??
    {}) as Partial<InvestmentTransactionListFilter>;

  let rows = db()
    .transactions.filter((t) => !t.is_deleted && INVESTMENT_KINDS.has(t.kind))
    .map((t) => ({ t, st: securityTxnOf(t.id) }))
    .filter((pair): pair is { t: Transaction; st: MockSecurityTxn } => pair.st !== undefined);

  if (filter.account_id) {
    const id = filter.account_id;
    rows = rows.filter(({ t }) => t.account_id === id || t.funding_account_id === id);
  }
  if (filter.instrument_id) {
    const id = filter.instrument_id;
    rows = rows.filter(({ st }) => st.instrument_id === id || st.to_instrument_id === id);
  }
  if (filter.kinds && filter.kinds.length > 0) {
    const kinds = new Set<TransactionKind>(filter.kinds);
    rows = rows.filter(({ t }) => kinds.has(t.kind));
  }
  if (filter.from) rows = rows.filter(({ t }) => t.date >= filter.from!);
  if (filter.to) rows = rows.filter(({ t }) => t.date <= filter.to!);

  rows = [...rows].sort(
    (a, b) => b.t.date.localeCompare(a.t.date) || b.t.created_at.localeCompare(a.t.created_at),
  );

  const total = rows.length;
  if (filter.page_size != null) {
    const page = filter.page ?? 1;
    const start = (page - 1) * filter.page_size;
    return {
      items: rows.slice(start, start + filter.page_size).map((p) => projectRow(p.t, p.st)),
      total,
    };
  }
  return { items: rows.map((p) => projectRow(p.t, p.st)), total };
}

function requireProjection(
  id: string,
  actions: ReadonlySet<MockSecurityTxn["action"]>,
  label: string,
): { t: Transaction; st: MockSecurityTxn } {
  const t = findActive(db().transactions, id, "交易");
  const st = securityTxnOf(id);
  if (!st || !actions.has(st.action)) {
    throw new Error(`替身后端：交易不存在或无${label}明细 ${id}`);
  }
  return { t, st };
}

export function getTransactionTrade(args: Record<string, unknown>): TransactionTrade {
  const id = str(args, "id");
  const { st } = requireProjection(id, new Set(["buy", "sell"]), "买卖");
  const instrument = instrumentOf(st.instrument_id);
  return {
    instrument_id: st.instrument_id,
    symbol: instrument?.symbol ?? "",
    instrument_name: instrument?.name ?? null,
    instrument_type: instrument?.type ?? "other",
    quantity: st.quantity ?? 0,
    price_cents: st.price_cents ?? 0,
    fee_cents: st.fee_cents,
  };
}

export function getTransactionConvert(args: Record<string, unknown>): TransactionConvert {
  const id = str(args, "id");
  const { t, st } = requireProjection(id, new Set(["convert"]), "转换");
  const out = instrumentOf(st.instrument_id);
  const inInstrument = st.to_instrument_id ? instrumentOf(st.to_instrument_id) : undefined;
  return {
    out_instrument_id: st.instrument_id,
    out_symbol: out?.symbol ?? "",
    out_instrument_name: out?.name ?? null,
    out_quantity: st.quantity ?? 0,
    out_amount_cents: st.out_amount_cents ?? 0,
    in_instrument_id: st.to_instrument_id ?? "",
    in_symbol: inInstrument?.symbol ?? "",
    in_instrument_name: inInstrument?.name ?? null,
    in_quantity: st.to_quantity ?? 0,
    in_amount_cents: st.in_amount_cents ?? 0,
    fee_cents: st.fee_cents,
    carried_cost_cents: t.amount_cents,
    currency_code: t.currency_code,
  };
}

export function getTransactionSplit(args: Record<string, unknown>): TransactionSplit {
  const id = str(args, "id");
  const { st } = requireProjection(id, new Set(["split"]), "份额调整");
  const instrument = instrumentOf(st.instrument_id);
  return {
    instrument_id: st.instrument_id,
    symbol: instrument?.symbol ?? "",
    instrument_name: instrument?.name ?? null,
    quantity: st.quantity ?? 0,
  };
}
