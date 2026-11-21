import { defineStore } from "pinia";
import { ref, computed } from "vue";
import { api } from "@ledger/api";
import { createPushFirstList } from "@/composables/push-first-list";
import {
  isFundingCandidateAccount,
  type Account,
  type Category,
  type Currency,
  type Insurer,
  type Merchant,
  type Syncable,
} from "@ledger/types";
import {
  rootCategories as pureRootCategories,
  categoryChildren as pureCategoryChildren,
  buildCategoryPathMap as pureBuildCategoryPathMap,
  buildCategoryTree as pureBuildCategoryTree,
  type CategoryTreeNode,
} from "@ledger/utils/category-tree";

function splitDeleted<T extends Syncable & { id: string }>(
  all: T[],
): { active: T[]; deleted: Map<string, T> } {
  return {
    active: all.filter((r) => !r.is_deleted),
    deleted: new Map(all.filter((r) => r.is_deleted).map((r) => [r.id, r])),
  };
}

export const useReferenceStore = defineStore("reference", () => {
  const currencies = ref<Currency[]>([]);
  const accounts = ref<Account[]>([]);
  const categories = ref<Category[]>([]);
  const merchants = ref<Merchant[]>([]);
  const insurers = ref<Insurer[]>([]);

  const deletedMerchants = ref(new Map<string, Merchant>());

  const deletedCategories = ref(new Map<string, Category>());

  const deletedInsurers = ref(new Map<string, Insurer>());

  const { status, version, refresh } = createPushFirstList(
    () =>
      Promise.all([
        api.listCurrencies(),
        api.listAccounts(),
        api.listCategories({ includeDeleted: true }),
        api.listMerchants({ includeDeleted: true }),
        api.listInsurers({ includeDeleted: true }),
      ]),
    ([cs, as, catsAll, msAll, insAll]) => {
      currencies.value = cs;
      accounts.value = as;
      const cats = splitDeleted(catsAll);
      categories.value = cats.active;
      deletedCategories.value = cats.deleted;
      const ms = splitDeleted(msAll);
      merchants.value = ms.active;
      deletedMerchants.value = ms.deleted;
      const ins = splitDeleted(insAll);
      insurers.value = ins.active;
      deletedInsurers.value = ins.deleted;
    },
  );

  const currencyMap = computed(() => {
    const m = new Map<string, Currency>();
    currencies.value.forEach((c) => m.set(c.code, c));
    return m;
  });

  const categoryMap = computed(() => {
    const m = new Map<string, Category>();
    deletedCategories.value.forEach((d) => m.set(d.id, d));
    categories.value.forEach((c) => m.set(c.id, c));
    return m;
  });

  const accountMap = computed(() => {
    const m = new Map<string, Account>();
    accounts.value.forEach((a) => m.set(a.id, a));
    return m;
  });

  const merchantMap = computed(() => {
    const m = new Map<string, Merchant>();
    deletedMerchants.value.forEach((d) => m.set(d.id, d));
    merchants.value.forEach((m2) => m.set(m2.id, m2));
    return m;
  });

  const merchantByName = computed(() => {
    const m = new Map<string, Merchant>();
    merchants.value.forEach((m2) => m.set(m2.name, m2));
    return m;
  });

  const insurerMap = computed(() => {
    const m = new Map<string, Insurer>();
    deletedInsurers.value.forEach((d) => m.set(d.id, d));
    insurers.value.forEach((i) => m.set(i.id, i));
    return m;
  });

  const insurerByName = computed(() => {
    const m = new Map<string, Insurer>();
    insurers.value.forEach((i) => m.set(i.name, i));
    return m;
  });

  const rootCategories = computed(() => pureRootCategories(categories.value));

  const expenseCategories = computed(() => categories.value.filter((c) => c.kind === "expense"));
  const incomeCategories = computed(() => categories.value.filter((c) => c.kind === "income"));

  const selectableAccounts = computed(() => accounts.value.filter((a) => !a.is_archived));

  const selectableAccountOptions = computed(() =>
    selectableAccounts.value.map((a) => ({ label: a.name, value: a.id })),
  );

  const investmentAccounts = computed(() => accounts.value.filter((a) => a.type === "investment"));

  const investmentAccountOptions = computed(() =>
    investmentAccounts.value.map((a) => ({ label: a.name, value: a.id })),
  );

  const selectableInvestmentAccountOptions = computed(() =>
    investmentAccounts.value
      .filter((a) => !a.is_archived)
      .map((a) => ({ label: a.name, value: a.id })),
  );

  const fundingCandidateAccounts = computed(() =>
    accounts.value.filter((a) => isFundingCandidateAccount(a.type) && !a.is_archived),
  );

  function categoryChildren(parentId: string): Category[] {
    return pureCategoryChildren(categories.value, parentId);
  }

  const categoryPathMap = computed(() => pureBuildCategoryPathMap(categoryMap.value.values()));

  function categoryPath(id: string | null | undefined): string {
    return categoryPathMap.value.get(id ?? "") ?? "";
  }

  function categoryDisplayName(id: string | null | undefined, fallback: string): string {
    return categoryPath(id) || fallback;
  }

  function treeCategoryOptions(kind: Category["kind"]): CategoryTreeNode[] {
    return pureBuildCategoryTree(categories.value, { kind });
  }

  function getCurrency(code: string): Currency | undefined {
    return currencyMap.value.get(code);
  }

  return {
    currencies,
    accounts,
    categories,
    merchants,
    insurers,
    deletedMerchants,
    deletedInsurers,
    status,
    version,
    currencyMap,
    categoryMap,
    accountMap,
    merchantMap,
    merchantByName,
    insurerMap,
    insurerByName,
    rootCategories,
    expenseCategories,
    incomeCategories,
    investmentAccounts,
    investmentAccountOptions,
    selectableAccounts,
    selectableAccountOptions,
    selectableInvestmentAccountOptions,
    fundingCandidateAccounts,
    categoryChildren,
    categoryPath,
    categoryDisplayName,
    treeCategoryOptions,
    refresh,
    getCurrency,
  };
});
