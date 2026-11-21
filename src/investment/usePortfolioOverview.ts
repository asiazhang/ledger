import { computed, onMounted, ref } from "vue";
import { api } from "@ledger/api";
import { useLoadable } from "@ledger/loadable";
import { useReferenceStore } from "@/stores/reference";
import type { Holding, InstrumentPriceChannel } from "@ledger/types";

export interface PortfolioRow {
  holdingId: string;
  accountId: string;
  accountName: string | null;
  instrumentId: string;
  symbol: string | null;
  instrumentName: string | null;
  quantity: number;
  costBasisCents: number;
  costCurrencyCode: string;
  latestPriceCents: number | null;
  latestPriceCurrencyCode: string | null;
  latestNavDate: string | null;
  marketValueCents: number | null;
  unrealizedPnlCents: number | null;
  nativeMarketValueCents: number | null;
  nativeUnrealizedPnlCents: number | null;
  valueCurrencyCode: string;
  priceChannel: InstrumentPriceChannel | null;
}

interface NativeTotal {
  cents: number | null;
  missingPriceCount: number;
  rateMissingCount: number;
}

export type StatCardValue = NativeTotal & {
  error: string | null;
};

export function rowStatCardValues(rows: PortfolioRow[]): {
  marketValue: StatCardValue;
  unrealizedPnl: StatCardValue;
} {
  return {
    marketValue: {
      ...nativeRowTotal(
        rows.map((r) => ({ cents: r.marketValueCents, nativeCents: r.nativeMarketValueCents })),
      ),
      error: null,
    },
    unrealizedPnl: {
      ...nativeRowTotal(
        rows.map((r) => ({
          cents: r.unrealizedPnlCents,
          nativeCents: r.nativeUnrealizedPnlCents,
        })),
      ),
      error: null,
    },
  };
}

function nativeRowTotal(rows: { cents: number | null; nativeCents: number | null }[]): NativeTotal {
  let cents: number | null = null;
  let missingPriceCount = 0;
  let rateMissingCount = 0;
  for (const r of rows) {
    if (r.cents === null) missingPriceCount++;
    else if (r.nativeCents === null) rateMissingCount++;
    else cents = (cents ?? 0) + r.nativeCents;
  }
  if (rateMissingCount > 0) cents = null;
  return { cents, missingPriceCount, rateMissingCount };
}

const INVESTED_INSTRUMENT_FETCH_LIMIT = 500;

export function usePortfolioOverview() {
  const reference = useReferenceStore();

  const rows = ref<PortfolioRow[]>([]);

  const { loading, error, run } = useLoadable(async () => {
    const [holdings, invested] = await Promise.all([
      api.listHoldings(),
      api.listInstruments({
        only_invested: true,
        page_size: INVESTED_INSTRUMENT_FETCH_LIMIT,
      }),
    ]);
    const instrumentMap = new Map(invested.items.map((i) => [i.id, i]));
    return holdings.map((h: Holding) => toRow(h, instrumentMap, reference.accountMap));
  });

  const { error: cumulativeError, run: runCumulative } = useLoadable(() =>
    api.cumulativePnlNativeTotal(),
  );
  const cumulativeLoaded = ref<{ cents: number; currencyCode: string } | null>(null);

  async function refresh() {
    const [rowResult, cumulativeResult] = await Promise.all([run(), runCumulative()]);
    if (rowResult !== null) rows.value = rowResult;
    if (cumulativeResult !== null) {
      cumulativeLoaded.value = {
        cents: cumulativeResult.total_cents,
        currencyCode: cumulativeResult.native_currency,
      };
    }
  }

  const statCards = computed(() => rowStatCardValues(rows.value));

  const cumulativePnl = computed<StatCardValue>(() => ({
    cents: cumulativeLoaded.value?.cents ?? null,
    missingPriceCount: 0,
    rateMissingCount: 0,
    error: cumulativeError.value,
  }));

  const nativeCurrency = computed(() => cumulativeLoaded.value?.currencyCode ?? null);

  onMounted(() => {
    void refresh();
  });

  return {
    rows,
    loading,
    error,
    statCards,
    cumulativePnl,
    nativeCurrency,
    refresh,
  };
}

interface InstrumentLike {
  symbol: string;
  name: string | null;
  price_channel: InstrumentPriceChannel;
}

interface AccountLike {
  name: string;
  currency_code: string;
}

function toRow(
  h: Holding,
  instrumentMap: Map<string, InstrumentLike>,
  accountMap: Map<string, AccountLike>,
): PortfolioRow {
  const inst = instrumentMap.get(h.instrument_id);
  const acct = accountMap.get(h.account_id);
  return {
    holdingId: h.id,
    accountId: h.account_id,
    accountName: acct?.name ?? null,
    instrumentId: h.instrument_id,
    symbol: inst?.symbol ?? null,
    instrumentName: inst?.name ?? null,
    quantity: h.quantity,
    costBasisCents: h.cost_basis_cents,
    costCurrencyCode: h.cost_currency_code,
    latestPriceCents: h.latest_price_cents,
    latestPriceCurrencyCode: h.latest_price_currency_code,
    latestNavDate: h.latest_nav_date,
    marketValueCents: h.market_value_cents,
    unrealizedPnlCents: h.unrealized_pnl_cents,
    nativeMarketValueCents: h.native_market_value_cents,
    nativeUnrealizedPnlCents: h.native_unrealized_pnl_cents,
    valueCurrencyCode: acct?.currency_code ?? h.cost_currency_code,
    priceChannel: inst?.price_channel ?? null,
  };
}
