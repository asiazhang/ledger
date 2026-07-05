import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { api } from '@/api'
import type { Account, Category, Currency } from '@/types'

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
    const m = new Map<number, Category>()
    categories.value.forEach((c) => m.set(c.id, c))
    return m
  })

  const accountMap = computed(() => {
    const m = new Map<number, Account>()
    accounts.value.forEach((a) => m.set(a.id, a))
    return m
  })

  const expenseCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'expense'),
  )
  const incomeCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'income'),
  )

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
    expenseCategories,
    incomeCategories,
    loaded,
    loadAll,
    loadCurrencies,
    loadAccounts,
    loadCategories,
    getCurrency,
  }
})
