import { computed, onScopeDispose, readonly, type Ref } from "vue";
import { storeToRefs } from "pinia";
import { useReferenceStore } from "@/stores/reference";
import { usePinnedAccountOptions } from "@/composables/useFormShared";
import { matchLabel } from "@ledger/utils/pinyin-filter";
import {
  rowStatCardValues,
  type StatCardValue,
  type PortfolioRow,
} from "@/investment/usePortfolioOverview";
import {
  HOLDINGS_PAGE_SIZE,
  useInvestmentsSessionStore,
  type HoldingsSorter,
  type NaiveUiSorterState,
} from "@/investment/investments-session";

export { HOLDINGS_PAGE_SIZE } from "@/investment/investments-session";
export type { HoldingsSortColumn, HoldingsSorter } from "@/investment/investments-session";

function nullsLast<T>(a: T | null, b: T | null, cmp: (x: T, y: T) => number): number {
  if (a === null && b === null) return 0;
  if (a === null) return 1;
  if (b === null) return -1;
  return cmp(a, b);
}

function comparePortfolioRows(a: PortfolioRow, b: PortfolioRow, sorter: HoldingsSorter): number {
  const key = sorter.columnKey === "market_value" ? "marketValueCents" : "unrealizedPnlCents";
  return nullsLast(a[key], b[key], (x, y) => {
    const ascending = x - y;
    return sorter.order === "descend" ? -ascending : ascending;
  });
}

export function sortHoldings(rows: PortfolioRow[], sorter: HoldingsSorter | null): PortfolioRow[] {
  const sorted = [...rows];
  if (sorter === null) {
    sorted.sort((a, b) => nullsLast(a.symbol, b.symbol, (x, y) => x.localeCompare(y)));
    return sorted;
  }
  return sorted.sort((a, b) => comparePortfolioRows(a, b, sorter));
}

export function holdingsSearchLabel(row: Pick<PortfolioRow, "symbol" | "instrumentName">): string {
  const name = row.instrumentName?.trim() || null;
  if (!name) return row.symbol ?? "";
  return row.symbol ? `${row.symbol} · ${name}` : name;
}

export function holdingMatchesSearch(row: PortfolioRow, query: string): boolean {
  const trimmed = query.trim();
  if (!trimmed) return true;
  return matchLabel(trimmed, holdingsSearchLabel(row));
}

export interface HoldingsFilterState {
  search: string;
  accountId: string | null;
}

export function filterHoldings(rows: PortfolioRow[], state: HoldingsFilterState): PortfolioRow[] {
  let out = rows;
  if (state.accountId !== null) {
    out = out.filter((r) => r.accountId === state.accountId);
  }
  if (state.search.trim()) {
    out = out.filter((r) => holdingMatchesSearch(r, state.search));
  }
  return out;
}

export interface UseHoldingsFilterReturn {
  readonly searchInput: Ref<string>;
  setSearch(input: string): void;
  readonly accountId: Ref<string | null>;
  setAccount(id: string | null): void;
  readonly sorter: Ref<HoldingsSorter | null>;
  setSorter(next: NaiveUiSorterState | NaiveUiSorterState[]): void;
  readonly filteredRows: Ref<PortfolioRow[]>;
  readonly page: Ref<number>;
  readonly statCards: Ref<{ marketValue: StatCardValue; unrealizedPnl: StatCardValue }>;
  readonly accountOptions: Ref<{ label: string; value: string }[]>;
}

export function useHoldingsFilter(rows: Ref<PortfolioRow[]>): UseHoldingsFilterReturn {
  const { selectableInvestmentAccountOptions } = storeToRefs(useReferenceStore());
  const session = useInvestmentsSessionStore();
  const {
    holdingsSearchInput,
    holdingsAccountId: accountId,
    holdingsSorter: sorter,
    holdingsPage: storedPage,
  } = storeToRefs(session);
  const searchInput = readonly(holdingsSearchInput);
  const accountOptions = usePinnedAccountOptions(accountId, selectableInvestmentAccountOptions);

  const filteredRows = computed(() =>
    sortHoldings(
      filterHoldings(rows.value, { search: session.holdingsSearch, accountId: accountId.value }),
      sorter.value,
    ),
  );
  const statCards = computed(() => rowStatCardValues(filteredRows.value));

  let lastReconciled: { stored: number; valid: number } | null = null;
  const page = computed<number>({
    get: () => {
      const valid = pageCount(filteredRows.value);
      const stored = storedPage.value;
      const clamped = Math.min(stored, valid);
      if (
        clamped !== stored &&
        (lastReconciled === null ||
          lastReconciled.stored !== stored ||
          lastReconciled.valid !== valid)
      ) {
        lastReconciled = { stored, valid };
        session.setPage(clamped);
      }
      return clamped;
    },
    set: (next) => {
      lastReconciled = null;
      session.setPage(next);
    },
  });

  onScopeDispose(() => {
    session.cancelPendingSearch();
  });

  return {
    searchInput,
    setSearch: session.setSearch,
    accountId,
    setAccount: session.setAccount,
    sorter,
    setSorter: session.setSorter,
    filteredRows,
    page,
    statCards,
    accountOptions,
  };
}

function pageCount(rows: PortfolioRow[]): number {
  return Math.max(1, Math.ceil(rows.length / HOLDINGS_PAGE_SIZE));
}
