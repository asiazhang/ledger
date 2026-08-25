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

/**
 * 参考数据（Reference Data）单一来源 store。
 *
 * 承载 `currencies / accounts / categories` 三张参考表及全部派生映射
 * （账户/分类/币种映射）与分类树逻辑。`useAppStore` 的参考数据 getters
 * 均委托到本 store，二者共享同一份状态。
 *
 * 注意：当前仍为「不缓存、不幂等」的拉取语义。账户/分类/币种可能被本地
 * HTTP API（AI 导入流程）外部修改，而 store 是内存态；各视图挂载时调用
 * `loadAll` 重新拉取，确保界面始终反映最新数据（本地 SQLite + IPC，开销可忽略）。
 */
export const useReferenceStore = defineStore('reference', () => {
  const currencies = ref<Currency[]>([])
  const accounts = ref<Account[]>([])
  const categories = ref<Category[]>([])

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
  }
})
