import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import type { Category, Currency } from '@/types'
import type { CategoryTreeNode } from '@/utils/category-tree'
import { loadLocal, saveLocal } from '@/utils/storage'

export type Theme = 'dark' | 'light'

export const useAppStore = defineStore('app', () => {
  // 参考数据（currencies/accounts/categories）及全部派生映射、分类树逻辑、
  // 加载函数已迁至 useReferenceStore（单一来源）。此处保留既有公开面，
  // 全部委托到新 store，二者共享同一份状态；现有消费者零改动。
  const reference = useReferenceStore()

  const currencies = computed(() => reference.currencies)
  const accounts = computed(() => reference.accounts)
  const categories = computed(() => reference.categories)

  const theme = ref<Theme>(loadLocal<Theme>('appearance', 'dark'))
  const defaultCurrency = ref<string>(loadLocal<string>('default_currency', 'CNY'))
  const backupDir = ref<string>(loadLocal<string>('backup_dir', ''))
  const backupMaxCount = ref<number>(loadLocal<number>('backup_max_count', 30))

  const currencyMap = computed(() => reference.currencyMap)
  const categoryMap = computed(() => reference.categoryMap)
  const accountMap = computed(() => reference.accountMap)

  const rootCategories = computed(() => reference.rootCategories)

  const expenseCategories = computed(() => reference.expenseCategories)
  const incomeCategories = computed(() => reference.incomeCategories)

  function categoryChildren(parentId: string): Category[] {
    return reference.categoryChildren(parentId)
  }

  function categoryPath(id: string | null | undefined): string {
    return reference.categoryPath(id)
  }

  function treeCategoryOptions(kind: Category['kind']): CategoryTreeNode[] {
    return reference.treeCategoryOptions(kind)
  }

  async function loadAll() {
    return reference.loadAll()
  }

  async function loadCurrencies() {
    return reference.loadCurrencies()
  }
  async function loadAccounts() {
    return reference.loadAccounts()
  }
  async function loadCategories() {
    return reference.loadCategories()
  }

  function getCurrency(code: string): Currency | undefined {
    return reference.getCurrency(code)
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

  function setBackupMaxCount(n: number) {
    backupMaxCount.value = n
    saveLocal('backup_max_count', n)
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
    backupMaxCount,
    setTheme,
    setDefaultCurrency,
    setBackupDir,
    setBackupMaxCount,
  }
})
