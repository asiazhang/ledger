import { defineStore } from "pinia";
import { reactive, readonly, ref, toRaw, watch } from "vue";
import { useReferenceStore } from "@/stores/reference";
import type { TransactionKind } from "@ledger/types";

export const UNCATEGORIZED_ONLY = "none";

const CATEGORY_DRILLDOWN_KIND_TOKENS = [
  "expense",
  "refund",
] as const satisfies readonly TransactionKind[];
export const CATEGORY_DRILLDOWN_KINDS = CATEGORY_DRILLDOWN_KIND_TOKENS.join(",");

const MERCHANT_DRILLDOWN_KIND_TOKENS = [
  "expense",
  "refund",
] as const satisfies readonly TransactionKind[];
export const MERCHANT_DRILLDOWN_KINDS = MERCHANT_DRILLDOWN_KIND_TOKENS.join(",");

export const MAIN_LIST_KINDS = [
  "income",
  "expense",
  "transfer",
  "refund",
] as const satisfies readonly TransactionKind[];

export function resolveRequestKinds(kinds: readonly TransactionKind[] | null): TransactionKind[] {
  return kinds?.length ? [...kinds] : [...MAIN_LIST_KINDS];
}

export function normalizeKindSelection(
  values: readonly TransactionKind[] | null,
): TransactionKind[] | null {
  if (!values?.length) return null;
  if (MAIN_LIST_KINDS.every((k) => values.includes(k))) return null;
  return [...values];
}

const DATE_PARAM_PATTERN = /^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])$/;

const TRANSACTION_PAGE_SIZE_DEFAULT = 20;

export interface TransactionFilters {
  dateFrom: string | null;
  dateTo: string | null;
  involvingAccountId: string | null;
  merchantId: string | null;
  categoryId: string | null;
  kinds: readonly TransactionKind[] | null;
}

export type TransactionFilterPatch = {
  [K in keyof TransactionFilters]?: TransactionFilters[K];
};

export type TransactionUrlQuery = Readonly<Record<string, unknown>>;

const DEFAULT_FILTERS: TransactionFilters = {
  dateFrom: null,
  dateTo: null,
  involvingAccountId: null,
  merchantId: null,
  categoryId: null,
  kinds: null,
};

type UrlParamCheck =
  | {
      readonly mapKey: "accountMap" | "merchantMap" | "categoryMap";
      readonly reservedValues?: ReadonlyArray<string>;
    }
  | { readonly pattern: RegExp }
  | { readonly literalSet: ReadonlyArray<TransactionKind> };

interface UrlParamDef {
  readonly queryKey: "account" | "merchant" | "category" | "kinds" | "dateFrom" | "dateTo";
  readonly field: keyof TransactionFilters;
  readonly check: UrlParamCheck;
  readonly toPatch: (value: string | null) => TransactionFilterPatch;
}

type UrlParamEntry = UrlParamDef & {
  raw: string | null;
  consumed: boolean;
  manualTouched: boolean;
};

const URL_PARAM_TABLE: ReadonlyArray<UrlParamDef> = [
  {
    queryKey: "account",
    field: "involvingAccountId",
    check: { mapKey: "accountMap" },
    toPatch: (value) => ({ involvingAccountId: value }),
  },
  {
    queryKey: "merchant",
    field: "merchantId",
    check: { mapKey: "merchantMap" },
    toPatch: (value) => ({ merchantId: value }),
  },
  {
    queryKey: "category",
    field: "categoryId",
    check: { mapKey: "categoryMap", reservedValues: [UNCATEGORIZED_ONLY] },
    toPatch: (value) => ({ categoryId: value }),
  },
  {
    queryKey: "kinds",
    field: "kinds",
    check: { literalSet: MAIN_LIST_KINDS },
    toPatch: (value) => ({ kinds: value ? (value.split(",") as TransactionKind[]) : null }),
  },
  {
    queryKey: "dateFrom",
    field: "dateFrom",
    check: { pattern: DATE_PARAM_PATTERN },
    toPatch: (value) => ({ dateFrom: value }),
  },
  {
    queryKey: "dateTo",
    field: "dateTo",
    check: { pattern: DATE_PARAM_PATTERN },
    toPatch: (value) => ({ dateTo: value }),
  },
];

export const useTransactionsSessionStore = defineStore("transactions-session", () => {
  const reference = useReferenceStore();

  const filters = reactive<TransactionFilters>({ ...DEFAULT_FILTERS });
  const page = ref(1);
  const pageSize = ref(TRANSACTION_PAGE_SIZE_DEFAULT);
  const refreshVersion = ref(0);

  const urlParams: UrlParamEntry[] = URL_PARAM_TABLE.map((def) => ({
    ...def,
    raw: null,
    consumed: false,
    manualTouched: false,
  }));

  const urlManagedFields: ReadonlySet<keyof TransactionFilters> = new Set(
    URL_PARAM_TABLE.map((def) => def.field),
  );

  function isAtDefault(): boolean {
    return (
      !Object.values(filters).some((v) => v !== null) &&
      page.value === 1 &&
      pageSize.value === TRANSACTION_PAGE_SIZE_DEFAULT
    );
  }

  function beginVisit() {
    urlParams.forEach((e) => {
      e.raw = null;
      e.consumed = false;
      e.manualTouched = false;
    });
  }

  function apply() {
    page.value = 1;
    refreshVersion.value += 1;
  }

  function isSameValue(a: unknown, b: unknown): boolean {
    if (a === b) return true;
    return (
      a !== null && b !== null && typeof a === "object" && typeof b === "object" && toRaw(a) === b
    );
  }

  function mutate(patch: TransactionFilterPatch, manual: boolean) {
    let changed = false;
    (Object.keys(patch) as Array<keyof TransactionFilters>).forEach((key) => {
      const value = patch[key];
      if (
        value === undefined ||
        isSameValue((filters as Record<keyof TransactionFilters, unknown>)[key], value)
      )
        return;
      (filters as Record<keyof TransactionFilters, unknown>)[key] = value;
      changed = true;
      if (manual && urlManagedFields.has(key)) {
        const entry = urlParams.find((e) => e.field === key);
        if (entry) entry.manualTouched = true;
      }
    });
    if (changed) apply();
  }

  function setFilter(patch: TransactionFilterPatch) {
    mutate(patch, true);
  }

  function resetFilters() {
    if (isAtDefault()) return;
    Object.assign(filters, DEFAULT_FILTERS);
    pageSize.value = TRANSACTION_PAGE_SIZE_DEFAULT;
    urlParams.forEach((e) => {
      e.manualTouched = true;
    });
    apply();
  }

  function refresh() {
    apply();
  }

  function afterRowDelete(remainingOnPage: number) {
    if (remainingOnPage === 0 && page.value > 1) {
      page.value -= 1;
    }
    refreshVersion.value += 1;
  }

  function resolveValue(entry: UrlParamEntry): string | null {
    if (entry.raw === null) return null;
    const check = entry.check;
    if ("mapKey" in check) {
      if (check.reservedValues?.includes(entry.raw)) return entry.raw;
      return reference[check.mapKey].has(entry.raw) ? entry.raw : null;
    }
    if ("literalSet" in check) {
      return entry.raw.split(",").every((p) => check.literalSet.includes(p as TransactionKind))
        ? entry.raw
        : null;
    }
    return check.pattern.test(entry.raw) ? entry.raw : null;
  }

  function isValidRaw(entry: UrlParamEntry): boolean {
    return resolveValue(entry) !== null;
  }

  function otherHasValidParam(entry: UrlParamEntry): boolean {
    return urlParams.some((e) => e !== entry && isValidRaw(e));
  }

  function settleEntry(entry: UrlParamEntry) {
    if (entry.consumed) return;
    if (entry.raw !== null && reference.status !== "ready") return;
    entry.consumed = true;
    if (entry.manualTouched) return;
    const next = resolveValue(entry);
    if (
      next === null &&
      !otherHasValidParam(entry) &&
      (filters.dateFrom !== null || filters.dateTo !== null || filters.kinds?.length)
    ) {
      mutate({ dateFrom: null, dateTo: null, kinds: null }, false);
    }
    mutate(entry.toPatch(next), false);
  }

  function syncUrlQuery(query: TransactionUrlQuery) {
    const changed = urlParams.filter((entry) => {
      const value = query[entry.queryKey];
      const raw = typeof value === "string" ? value : null;
      if (raw === entry.raw) return false;
      entry.raw = raw;
      entry.consumed = false;
      entry.manualTouched = false;
      return true;
    });
    changed.forEach(settleEntry);
  }

  watch(
    () => reference.status,
    (status) => {
      if (status !== "ready") return;
      urlParams.forEach((entry) => {
        if (entry.raw !== null) settleEntry(entry);
      });
    },
  );

  return {
    filters: readonly(filters),
    page,
    pageSize,
    refreshVersion,
    beginVisit,
    setFilter,
    resetFilters,
    refresh,
    afterRowDelete,
    syncUrlQuery,
  };
});
