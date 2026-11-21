import type { Ref } from "vue";
import { storeToRefs } from "pinia";
import { useTransactionsSessionStore } from "@/transaction/transactions-session";
import type {
  TransactionFilters,
  TransactionFilterPatch,
  TransactionUrlQuery,
} from "@/transaction/transactions-session";

export {
  UNCATEGORIZED_ONLY,
  CATEGORY_DRILLDOWN_KINDS,
  MERCHANT_DRILLDOWN_KINDS,
  MAIN_LIST_KINDS,
  resolveRequestKinds,
  normalizeKindSelection,
} from "@/transaction/transactions-session";

export interface UseTransactionFilterReturn {
  readonly filters: Readonly<TransactionFilters>;
  readonly page: Ref<number>;
  readonly pageSize: Ref<number>;
  readonly refreshVersion: Ref<number>;
  setFilter(patch: TransactionFilterPatch): void;
  resetFilters(): void;
  refresh(): void;
  afterRowDelete(remainingOnPage: number): void;
  syncUrlQuery(query: TransactionUrlQuery): void;
}

export function useTransactionFilter(): UseTransactionFilterReturn {
  const store = useTransactionsSessionStore();
  store.beginVisit();
  const { page, pageSize, refreshVersion } = storeToRefs(store);
  return {
    filters: store.filters,
    page,
    pageSize,
    refreshVersion,
    setFilter: store.setFilter,
    resetFilters: store.resetFilters,
    refresh: store.refresh,
    afterRowDelete: store.afterRowDelete,
    syncUrlQuery: store.syncUrlQuery,
  };
}
