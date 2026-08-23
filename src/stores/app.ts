import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { api } from '@/api'
import type { Account, Category, Currency } from '@/types'
import {
  rootCategories as pureRootCategories,
  categoryChildren as pureCategoryChildren,
  categoryPath as pureCategoryPath,
  buildCategoryTree as pureBuildCategoryTree,
  type CategoryTreeNode,
} from '@/utils/category-tree'
import { loadLocal, saveLocal } from '@/utils/storage'

export type Theme = 'dark' | 'light'

export const useAppStore = defineStore('app', () => {
  const currencies = ref<Currency[]>([])
  const accounts = ref<Account[]>([])
  const categories = ref<Category[]>([])

  const theme = ref<Theme>(loadLocal<Theme>('appearance', 'dark'))
  const defaultCurrency = ref<string>(loadLocal<string>('default_currency', 'CNY'))
  const backupDir = ref<string>(loadLocal<string>('backup_dir', ''))

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
    return pureBuildCategoryTree(categories.value, { kind })
  }

  // 注意：不缓存、不幂等。账户/分类/币种可能被本地 HTTP API（AI 导入流程）
  // 外部修改，而 store 是内存态；各视图挂载时调用本函数重新拉取，
  // 确保界面始终反映最新数据（本地 SQLite + IPC，开销可忽略）。
  async function loadAll() {
    await Promise.all([loadCurrencies(), loadAccounts(), loadCategories()])
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

  function setBackupDir(dir: string) {
    backupDir.value = dir
    saveLocal('backup_dir', dir)
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
    loadAll,
    loadCurrencies,
    loadAccounts,
    loadCategories,
    getCurrency,
    theme,
    defaultCurrency,
    backupDir,
    setTheme,
    setDefaultCurrency,
    setBackupDir,
  }
})
