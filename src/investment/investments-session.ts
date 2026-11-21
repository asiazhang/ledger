import { defineStore } from "pinia";
import { readonly, ref, watch } from "vue";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import type { TransactionKind } from "@ledger/types";

export type HoldingsSortColumn = "market_value" | "unrealized_pnl";
type HoldingsSortOrder = "ascend" | "descend";

export interface HoldingsSorter {
  columnKey: HoldingsSortColumn;
  order: HoldingsSortOrder;
}

export interface NaiveUiSorterState {
  columnKey: string | number;
  order: "ascend" | "descend" | false;
}

export const HOLDINGS_PAGE_SIZE = 20;

export const PNL_PAGE_SIZE = 8;

const INVESTMENTS_DEFAULT_TAB = "overview";

export const LEDGER_TAB_PAGE_SIZE_DEFAULT = 20;

export type TrendRangePreset = "1m" | "3m" | "1y" | "3y" | "5y" | "all";

export const TREND_PRESET_DEFAULT: TrendRangePreset = "1y";

export const useInvestmentsSessionStore = defineStore("investments-session", () => {
  const activeTab = ref<string>(INVESTMENTS_DEFAULT_TAB);

  const holdingsSearchInput = ref("");
  const holdingsSearch = ref("");
  const holdingsAccountId = ref<string | null>(null);
  const holdingsSorter = ref<HoldingsSorter | null>(null);
  const holdingsPage = ref(1);
  const pnlYearPage = ref(1);
  const pnlAccountPage = ref(1);

  const detailKinds = ref<TransactionKind[] | null>(null);
  const detailPage = ref(1);
  const detailPageSize = ref(LEDGER_TAB_PAGE_SIZE_DEFAULT);

  const detailAccountId = ref<string | null>(null);
  const detailInstrumentId = ref<string | null>(null);
  const detailDateFrom = ref<string | null>(null);
  const detailDateTo = ref<string | null>(null);

  const trendPreset = ref<TrendRangePreset>(TREND_PRESET_DEFAULT);

  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  function setSearch(input: string) {
    holdingsSearchInput.value = input;
    clearTimeout(searchTimer);
    const next = input.trim();
    if (next === holdingsSearch.value) {
      searchTimer = undefined;
      return;
    }
    searchTimer = setTimeout(() => {
      searchTimer = undefined;
      holdingsSearch.value = next;
    }, SEARCH_DEBOUNCE_MS);
  }

  function cancelPendingSearch() {
    clearTimeout(searchTimer);
    searchTimer = undefined;
    holdingsSearchInput.value = holdingsSearch.value;
  }

  function setAccount(id: string | null) {
    holdingsAccountId.value = id;
  }

  function setSorter(next: NaiveUiSorterState | NaiveUiSorterState[]) {
    const single = Array.isArray(next) ? next[0] : next;
    let resolved: HoldingsSorter | null = null;
    if (
      single &&
      single.order !== false &&
      (single.columnKey === "market_value" || single.columnKey === "unrealized_pnl")
    ) {
      resolved = { columnKey: single.columnKey, order: single.order };
    }
    if (
      holdingsSorter.value?.columnKey === resolved?.columnKey &&
      holdingsSorter.value?.order === resolved?.order
    ) {
      return;
    }
    holdingsSorter.value = resolved;
  }

  function setPage(next: number) {
    holdingsPage.value = next;
  }

  function setPnlYearPage(next: number) {
    pnlYearPage.value = next;
  }

  function setPnlAccountPage(next: number) {
    pnlAccountPage.value = next;
  }

  function setDetailKinds(kinds: TransactionKind[] | null) {
    const next = kinds?.length ? [...kinds] : null;
    const current = detailKinds.value;
    const same =
      next === current ||
      (next !== null &&
        current !== null &&
        next.length === current.length &&
        next.every((k) => current.includes(k)));
    if (same) return;
    detailKinds.value = next;
  }

  function setDetailPage(next: number) {
    detailPage.value = next;
  }

  function setDetailPageSize(size: number) {
    if (size === detailPageSize.value) return;
    detailPageSize.value = size;
    detailPage.value = 1;
  }

  function setDetailAccount(id: string | null) {
    if (id === detailAccountId.value) return;
    detailAccountId.value = id;
  }

  function setDetailInstrument(id: string | null) {
    if (id === detailInstrumentId.value) return;
    detailInstrumentId.value = id;
  }

  function setDetailDateRange(range: [string, string] | null) {
    const from = range?.[0] ?? null;
    const to = range?.[1] ?? null;
    if (from === detailDateFrom.value && to === detailDateTo.value) return;
    detailDateFrom.value = from;
    detailDateTo.value = to;
  }

  function resetDetailFilters() {
    setDetailKinds(null);
    setDetailAccount(null);
    setDetailInstrument(null);
    setDetailDateRange(null);
  }

  watch(
    [holdingsSearch, holdingsAccountId, holdingsSorter],
    () => {
      holdingsPage.value = 1;
    },
    { flush: "sync" },
  );

  watch(
    [detailKinds, detailAccountId, detailInstrumentId, detailDateFrom, detailDateTo],
    () => {
      detailPage.value = 1;
    },
    { flush: "sync" },
  );

  function setActiveTab(tab: string) {
    activeTab.value = tab;
  }

  function setTrendPreset(preset: TrendRangePreset) {
    trendPreset.value = preset;
  }

  function resetToDefault() {
    clearTimeout(searchTimer);
    searchTimer = undefined;
    activeTab.value = INVESTMENTS_DEFAULT_TAB;
    holdingsSearchInput.value = "";
    holdingsSearch.value = "";
    holdingsAccountId.value = null;
    holdingsSorter.value = null;
    holdingsPage.value = 1;
    pnlYearPage.value = 1;
    pnlAccountPage.value = 1;
    trendPreset.value = TREND_PRESET_DEFAULT;
    detailKinds.value = null;
    detailPage.value = 1;
    detailPageSize.value = LEDGER_TAB_PAGE_SIZE_DEFAULT;
    detailAccountId.value = null;
    detailInstrumentId.value = null;
    detailDateFrom.value = null;
    detailDateTo.value = null;
  }

  return {
    activeTab: readonly(activeTab),
    holdingsSearchInput: readonly(holdingsSearchInput),
    holdingsSearch: readonly(holdingsSearch),
    holdingsAccountId: readonly(holdingsAccountId),
    holdingsSorter: readonly(holdingsSorter),
    holdingsPage: readonly(holdingsPage),
    pnlYearPage: readonly(pnlYearPage),
    pnlAccountPage: readonly(pnlAccountPage),
    detailKinds: readonly(detailKinds),
    detailPage: readonly(detailPage),
    detailPageSize: readonly(detailPageSize),
    detailAccountId: readonly(detailAccountId),
    detailInstrumentId: readonly(detailInstrumentId),
    detailDateFrom: readonly(detailDateFrom),
    detailDateTo: readonly(detailDateTo),
    trendPreset: readonly(trendPreset),
    setActiveTab,
    setSearch,
    cancelPendingSearch,
    setAccount,
    setSorter,
    setPage,
    setPnlYearPage,
    setPnlAccountPage,
    setDetailKinds,
    setDetailPage,
    setDetailPageSize,
    setDetailAccount,
    setDetailInstrument,
    setDetailDateRange,
    resetDetailFilters,
    setTrendPreset,
    resetToDefault,
  };
});
