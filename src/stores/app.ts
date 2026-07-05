import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { api } from '@/api'
import type { Account, Category, Currency } from '@/types'

/** NTreeSelect 树形节点（key/label/children）。 */
export interface CategoryTreeNode {
  key: number
  label: string
  children?: CategoryTreeNode[]
  [key: string]: unknown
}

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

  // 顶级分类（parent_id 为空）
  const rootCategories = computed(() =>
    categories.value.filter((c) => c.parent_id == null),
  )

  const expenseCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'expense'),
  )
  const incomeCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'income'),
  )

  /** 取某父分类的直系子分类。 */
  function categoryChildren(parentId: number): Category[] {
    return categories.value.filter((c) => c.parent_id === parentId)
  }

  /** 分类层级路径文本：二级显示“父 > 子”，顶级显示单级名；找不到返回空串。 */
  function categoryPath(id: number | null | undefined): string {
    if (id == null) return ''
    const cat = categoryMap.value.get(id)
    if (!cat) return ''
    if (cat.parent_id == null) return cat.name
    const parent = categoryMap.value.get(cat.parent_id)
    return parent ? `${parent.name} > ${cat.name}` : cat.name
  }

  /** 构造 NTreeSelect 树形 options：顶级可选中，有子分类的顶级展开二级。 */
  function treeCategoryOptions(kind: Category['kind']): CategoryTreeNode[] {
    return rootCategories.value
      .filter((c) => c.kind === kind)
      .map((root) => {
        const children = categoryChildren(root.id)
        const node: CategoryTreeNode = { key: root.id, label: root.name }
        if (children.length > 0) {
          node.children = children.map((c) => ({ key: c.id, label: c.name }))
        }
        return node
      })
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
