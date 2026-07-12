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

function loadLocal<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    if (raw !== null) return JSON.parse(raw) as T
  } catch { /* ignore */ }
  return fallback
}

function saveLocal<T>(key: string, value: T) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch { /* ignore */ }
}

export type Theme = 'dark' | 'light'

export const useAppStore = defineStore('app', () => {
  const currencies = ref<Currency[]>([])
  const accounts = ref<Account[]>([])
  const categories = ref<Category[]>([])
  const loaded = ref(false)

  const theme = ref<Theme>(loadLocal<Theme>('appearance', 'dark'))
  const defaultCurrency = ref<string>(loadLocal<string>('default_currency', 'CNY'))

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

  function setTheme(t: Theme) {
    theme.value = t
    saveLocal('appearance', t)
  }

  function setDefaultCurrency(code: string) {
    defaultCurrency.value = code
    saveLocal('default_currency', code)
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
    theme,
    defaultCurrency,
    setTheme,
    setDefaultCurrency,
  }
})
