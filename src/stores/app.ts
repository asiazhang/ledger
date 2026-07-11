import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { api } from '@/api'
import type { Account, Category, Currency } from '@/types'
import {
  rootCategories as pureRootCategories,
  categoryChildren as pureCategoryChildren,
  categoryPath as pureCategoryPath,
  treeCategoryOptions as pureTreeCategoryOptions,
  type CategoryTreeNode,
} from '@/types/category'

export const useAppStore = defineStore('app', () => {
  const currencies = ref<Currency[]>([])
  const accounts = ref<Account[]>([])
  const categories = ref<Category[]>([])
  const loaded = ref(false)

  const currencyMap = computed(() => {
    const m = new Map<string, Currency>()
    currencies.value.forEach((c) => m.set(c.code, c))
    return m
  })

  const categoryMap = computed(() => {
    const m = new Map<string, Category>()
    categories.value.forEach((c) => m.set(c.id, c))
    return m
  })

  const accountMap = computed(() => {
    const m = new Map<string, Account>()
    accounts.value.forEach((a) => m.set(a.id, a))
    return m
  })

  const rootCategories = computed(() => pureRootCategories(categories.value))

  const expenseCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'expense'),
  )
  const incomeCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'income'),
  )

  function categoryChildren(parentId: string): Category[] {
    return pureCategoryChildren(categories.value, parentId)
  }

  function categoryPath(id: string | null | undefined): string {
    return pureCategoryPath(categories.value, id)
  }

  function treeCategoryOptions(kind: Category['kind']): CategoryTreeNode[] {
    return pureTreeCategoryOptions(categories.value, kind)
  }

  async function loadAll() {
    if (loaded.value) return
    await Promise.all([loadCurrencies(), loadAccounts(), loadCategories()])
    loaded.value = true
  }

  async function loadCurrencies() {
    currencies.value = await api.listCurrencies()
  }
  async function loadAccounts() {
    accounts.value = await api.listAccounts()
  }
  async function loadCategories() {
    categories.value = await api.listCategories()
  }

  function getCurrency(code: string): Currency | undefined {
    return currencyMap.value.get(code)
  }

  return {
    currencies,
    accounts,
    categories,
    currencyMap,
    categoryMap,
    accountMap,
    rootCategories,
    expenseCategories,
    incomeCategories,
    categoryChildren,
    categoryPath,
    treeCategoryOptions,
    loaded,
    loadAll,
    loadCurrencies,
    loadAccounts,
    loadCategories,
    getCurrency,
  }
})
