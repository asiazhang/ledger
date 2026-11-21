import type {
  AccountMwr,
  AccountPnl,
  CurrencyCumulativePnl,
  CumulativePnlNativeTotal,
  Holding,
  InstrumentMwr,
  InvestmentOverview,
  MoneyWeightedReturnSummary,
  PortfolioValueTrend,
  PortfolioTrendPoint,
  PriceStaleness,
  RealizedPnlSummary,
  SyncInstrumentInfoResult,
  YearPnl,
} from "@ledger/types";
import { accountBalanceCents } from "./ledger";
import { argsOf, db, obj } from "./state";
import { emitMockEvent } from "./tauri/event";

export function listHoldings(): Holding[] {
  return [...db().holdings].sort(
    (a, b) =>
      a.account_id.localeCompare(b.account_id) || a.instrument_id.localeCompare(b.instrument_id),
  );
}

function sellRealizedCents(
  t: { amount_cents: number; account_id: string | null; is_deleted: boolean },
  st: { instrument_id: string; quantity: number | null; fee_cents: number },
): number {
  const holding = db().holdings.find(
    (h) => h.instrument_id === st.instrument_id && h.account_id === t.account_id,
  );
  const avgCost = holding && holding.quantity > 0 ? holding.cost_basis_cents / holding.quantity : 0;
  return t.amount_cents - (st.quantity ?? 0) * avgCost - st.fee_cents;
}

interface SellEvent {
  year: string;
  account_id: string | null;
  instrument_id: string;
  currency_code: string;
  realized: number;
}

interface DividendEvent {
  year: string;
  account_id: string | null;
  instrument_id: string;
  currency_code: string;
  amount: number;
}

function legEvents(filter?: { account_id?: string | null; instrument_id?: string | null }): {
  sells: SellEvent[];
  dividends: DividendEvent[];
} {
  const sells: SellEvent[] = [];
  const dividends: DividendEvent[] = [];
  for (const t of db().transactions) {
    if (t.is_deleted) continue;
    if (filter?.account_id && t.account_id !== filter.account_id) continue;
    const st = db().securityTransactions.find((s) => s.transaction_id === t.id);
    if (!st) continue;
    if (filter?.instrument_id && st.instrument_id !== filter.instrument_id) continue;
    if (t.kind === "sell") {
      sells.push({
        year: t.date.slice(0, 4),
        account_id: t.account_id,
        instrument_id: st.instrument_id,
        currency_code: t.currency_code,
        realized: sellRealizedCents(t, st),
      });
    } else if (t.kind === "dividend") {
      dividends.push({
        year: t.date.slice(0, 4),
        account_id: t.account_id,
        instrument_id: st.instrument_id,
        currency_code: t.currency_code,
        amount: t.amount_cents,
      });
    }
  }
  return { sells, dividends };
}

export function moneyWeightedReturnSummary(): MoneyWeightedReturnSummary {
  const RATES: Record<string, number | null> = {
    "mock-inst-custom": null,
    "mock-inst-etf": -0.0005,
    "mock-inst-fund": 0.05,
    "mock-inst-hk": 0.12,
  };
  const by_instrument: InstrumentMwr[] = db()
    .holdings.filter((h) => h.latest_price_cents !== null)
    .sort((a, b) => a.instrument_id.localeCompare(b.instrument_id))
    .map((h) => ({
      account_id: h.account_id,
      instrument_id: h.instrument_id,
      currency_code: h.cost_currency_code,
      basis:
        h.instrument_id === "mock-inst-etf" ? ("annualized" as const) : ("cumulative" as const),
      rate: RATES[h.instrument_id] ?? null,
    }));

  const by_account: AccountMwr[] = [
    {
      account_id: "mock-acc-invest",
      account_name: db().accounts.find((a) => a.id === "mock-acc-invest")?.name ?? "",
      currency_code: "CNY",
      basis: "cumulative",
      rate: 0.08,
    },
  ];

  const total = [
    { currency_code: "CNY", basis: "cumulative" as const, rate: 0.04 },
    { currency_code: "HKD", basis: "cumulative" as const, rate: 0.12 },
  ];

  return { by_instrument, by_account, total };
}

function cmpCodepoint(a: string, b: string): number {
  const ai = a[Symbol.iterator]();
  const bi = b[Symbol.iterator]();
  for (;;) {
    const x = ai.next();
    const y = bi.next();
    if (x.done || y.done) return x.done === y.done ? 0 : x.done ? -1 : 1;
    const d = x.value.codePointAt(0)! - y.value.codePointAt(0)!;
    if (d !== 0) return d < 0 ? -1 : 1;
  }
}

export function realizedPnlSummary(args: Record<string, unknown>): RealizedPnlSummary {
  const filter = (obj<{ account_id?: string | null; instrument_id?: string | null } | null>(
    argsOf(args),
    "filter",
  ) ?? {}) as { account_id?: string | null; instrument_id?: string | null };
  const native = db().settings.base_currency;
  const { sells, dividends } = legEvents(filter);

  const years = new Set([...sells.map((s) => s.year), ...dividends.map((d) => d.year)]);
  const by_year: YearPnl[] = [...years].sort().map((year) => {
    const realized = sells.filter((s) => s.year === year).reduce((sum, s) => sum + s.realized, 0);
    const dividend = dividends.filter((d) => d.year === year).reduce((sum, d) => sum + d.amount, 0);
    return {
      year,
      native_currency: native,
      realized_pnl_cents: realized,
      dividend_cents: dividend,
      realized_gain_cents: realized + dividend,
    };
  });

  const accounts = db().accounts;
  const accountId = (id: string | null): string => id ?? "";
  const accountIds = new Set([
    ...sells.map((s) => accountId(s.account_id)),
    ...dividends.map((d) => accountId(d.account_id)),
  ]);
  const by_account: AccountPnl[] = [...accountIds]
    .map((id) => {
      const realized = sells
        .filter((s) => accountId(s.account_id) === id)
        .reduce((sum, s) => sum + s.realized, 0);
      const dividend = dividends
        .filter((d) => accountId(d.account_id) === id)
        .reduce((sum, d) => sum + d.amount, 0);
      return {
        account_id: id,
        account_name: accounts.find((a) => a.id === id)?.name ?? "",
        native_currency: native,
        realized_pnl_cents: realized,
        dividend_cents: dividend,
        realized_gain_cents: realized + dividend,
      };
    })
    .sort(
      (a, b) =>
        cmpCodepoint(a.account_name, b.account_name) || cmpCodepoint(a.account_id, b.account_id),
    );

  return { by_year, by_account };
}

function cumulativeLegs(): Map<string, number> {
  const legs = new Map<string, number>();
  const add = (currency: string, cents: number): void => {
    legs.set(currency, (legs.get(currency) ?? 0) + cents);
  };
  for (const h of db().holdings) {
    if (h.unrealized_pnl_cents === null) continue;
    add(h.cost_currency_code, h.unrealized_pnl_cents);
  }
  const { sells, dividends } = legEvents();
  for (const s of sells) add(s.currency_code, s.realized);
  for (const d of dividends) add(d.currency_code, d.amount);
  return legs;
}

export function cumulativePnlSummary(): CurrencyCumulativePnl[] {
  return [...cumulativeLegs()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([currency_code, cents]) => ({ currency_code, cumulative_pnl_cents: cents }));
}

export function cumulativePnlNativeTotal(): CumulativePnlNativeTotal {
  const total = [...cumulativeLegs().values()].reduce((sum, cents) => sum + cents, 0);
  return { total_cents: total, native_currency: db().settings.base_currency };
}

export function portfolioValueTrend(args: Record<string, unknown>): PortfolioValueTrend {
  const range = (obj<{ start_date?: string | null; end_date?: string | null } | null>(
    argsOf(args),
    "filter",
  ) ?? {}) as { start_date?: string | null; end_date?: string | null };
  const points: PortfolioTrendPoint[] = db().portfolioTrendPoints.filter(
    (p) =>
      (range.start_date == null || p.date >= range.start_date) &&
      (range.end_date == null || p.date <= range.end_date),
  );
  return { currency_code: db().settings.base_currency, points };
}

export function instrumentPriceStaleness(): PriceStaleness {
  return { stale_count: 0, threshold_days: 7 };
}

export function syncInstrumentInfo(): SyncInstrumentInfoResult {
  const rows = db().instruments.filter((i) => !i.is_deleted);
  const synced = rows.filter((i) => i.price_channel !== "none").length;
  const skipped = rows.length - synced;
  if (synced > 0) emitMockEvent("ledger:prices-changed");
  return {
    synced,
    skipped,
    message: synced + skipped === 0 ? "暂无标的可同步" : `已同步 ${synced} 只，跳过 ${skipped} 只`,
    bulk_degraded: false,
  };
}

export function investmentOverview(): InvestmentOverview {
  const native = db().settings.base_currency;

  let marketValue: number | null = null;
  let unrealized: number | null = null;
  let missing = 0;
  for (const h of db().holdings) {
    if (h.native_market_value_cents === null || h.native_unrealized_pnl_cents === null) {
      missing++;
      continue;
    }
    marketValue = (marketValue ?? 0) + h.native_market_value_cents;
    unrealized = (unrealized ?? 0) + h.native_unrealized_pnl_cents;
  }

  const { sells, dividends } = legEvents();
  const realized = sells.reduce((sum, s) => sum + s.realized, 0);
  const dividend = dividends.reduce((sum, d) => sum + d.amount, 0);

  const investmentCash = db()
    .accounts.filter((a) => !a.is_deleted && !a.is_hidden && a.type === "investment")
    .reduce((sum, a) => sum + accountBalanceCents(a.id), 0);

  return {
    native_currency: native,
    investment_cash_cents: investmentCash,
    holdings_market_value_cents: marketValue ?? 0,
    investable_assets_cents: investmentCash + (marketValue ?? 0),
    total_market_value_cents: marketValue ?? 0,
    unrealized_pnl_cents: unrealized ?? 0,
    cumulative_pnl_cents: (unrealized ?? 0) + realized + dividend,
    missing_price_holding_count: missing,
    has_investment_account: db().accounts.some((a) => !a.is_deleted && a.type === "investment"),
  };
}
