import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import { useAppStore } from '@/stores/app'
import type { Account, Category, Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY',
    initial_balance_cents: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
  {
    id: 'acc-2', name: '招商银行', type: 'bank', currency_code: 'CNY',
    initial_balance_cents: 100000, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
]

const mockCategories: Category[] = [
  {
    id: 'cat-root', name: '餐饮', kind: 'expense', parent_id: null,
    icon: null, sort_order: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'cat-child', name: '外卖', kind: 'expense', parent_id: 'cat-root',
    icon: null, sort_order: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'cat-income', name: '工资', kind: 'income', parent_id: null,
    icon: null, sort_order: 0, created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]

function mockListCommands() {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListCommands()
  localStorage.clear()
})

describe('useReferenceStore', () => {
  it('初始状态为空', () => {
    const store = useReferenceStore()
    expect(store.currencies).toEqual([])
    expect(store.accounts).toEqual([])
    expect(store.categories).toEqual([])
  })

  it('loadAll 拉取三张参考表并填充响应式状态', async () => {
    const store = useReferenceStore()
    await store.loadAll()
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)
  })

  it('loadCurrencies/loadAccounts/loadCategories 可单独拉取', async () => {
    const store = useReferenceStore()
    await store.loadCurrencies()
    expect(store.currencies).toEqual(mockCurrencies)
    await store.loadAccounts()
    expect(store.accounts).toEqual(mockAccounts)
    await store.loadCategories()
    expect(store.categories).toEqual(mockCategories)
  })

  it('派生映射 currencyMap/accountMap/categoryMap 正确', async () => {
    const store = useReferenceStore()
    await store.loadAll()
    expect(store.currencyMap.get('USD')?.name).toBe('美元')
    expect(store.accountMap.get('acc-2')?.name).toBe('招商银行')
    expect(store.categoryMap.get('cat-child')?.name).toBe('外卖')
  })

  it('分类树派生：rootCategories/expenseCategories/incomeCategories', async () => {
    const store = useReferenceStore()
    await store.loadAll()
    expect(store.rootCategories.map((c) => c.id)).toEqual(['cat-root', 'cat-income'])
    expect(store.expenseCategories.map((c) => c.id)).toEqual(['cat-root', 'cat-child'])
    expect(store.incomeCategories.map((c) => c.id)).toEqual(['cat-income'])
  })

  it('categoryChildren/categoryPath/treeCategoryOptions 正确', async () => {
    const store = useReferenceStore()
    await store.loadAll()
    expect(store.categoryChildren('cat-root').map((c) => c.id)).toEqual(['cat-child'])
    expect(store.categoryPath('cat-child')).toBe('餐饮 > 外卖')
    expect(store.categoryPath('cat-root')).toBe('餐饮')
    expect(store.categoryPath(null)).toBe('')
    const expenseTree = store.treeCategoryOptions('expense')
    expect(expenseTree.map((n) => n.key)).toEqual(['cat-root'])
    expect(expenseTree[0].children?.map((c) => c.key)).toEqual(['cat-child'])
    expect(store.treeCategoryOptions('income').map((n) => n.key)).toEqual(['cat-income'])
  })

  it('getCurrency 按 code 返回币种', async () => {
    const store = useReferenceStore()
    await store.loadAll()
    expect(store.getCurrency('CNY')?.symbol).toBe('¥')
    expect(store.getCurrency('EUR')).toBeUndefined()
  })
})

describe('useAppStore 委托 useReferenceStore（单一来源）', () => {
  it('通过 useAppStore.loadAll 加载后 useReferenceStore 可见同一数据', async () => {
    const app = useAppStore()
    const reference = useReferenceStore()
    await app.loadAll()
    expect(reference.currencies).toEqual(mockCurrencies)
    expect(reference.accounts).toEqual(mockAccounts)
    expect(reference.categories).toEqual(mockCategories)
  })

  it('通过 useReferenceStore.loadAll 加载后 useAppStore 可见同一数据', async () => {
    const app = useAppStore()
    const reference = useReferenceStore()
    await reference.loadAll()
    expect(app.currencies).toEqual(mockCurrencies)
    expect(app.accounts).toEqual(mockAccounts)
    expect(app.categories).toEqual(mockCategories)
  })

  it('useAppStore 的派生映射与加载函数委托到 reference store', async () => {
    const app = useAppStore()
    await app.loadAll()
    expect(app.currencyMap.get('USD')?.name).toBe('美元')
    expect(app.accountMap.get('acc-2')?.name).toBe('招商银行')
    expect(app.categoryMap.get('cat-child')?.name).toBe('外卖')
    expect(app.categoryPath('cat-child')).toBe('餐饮 > 外卖')
    expect(app.categoryChildren('cat-root').map((c) => c.id)).toEqual(['cat-child'])
    expect(app.rootCategories.map((c) => c.id)).toEqual(['cat-root', 'cat-income'])
    expect(app.expenseCategories.map((c) => c.id)).toEqual(['cat-root', 'cat-child'])
    expect(app.incomeCategories.map((c) => c.id)).toEqual(['cat-income'])
    expect(app.treeCategoryOptions('expense').map((n) => n.key)).toEqual(['cat-root'])
    expect(app.getCurrency('CNY')?.symbol).toBe('¥')
  })

  it('reference store 数据变化后 app store 派生映射同步更新', async () => {
    const app = useAppStore()
    const reference = useReferenceStore()
    await app.loadAll()
    // 再次拉取不同数据，两边应同步反映同一份状态
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve([mockCurrencies[1]])
      if (cmd === 'list_accounts') return Promise.resolve([mockAccounts[0]])
      if (cmd === 'list_categories') return Promise.resolve([mockCategories[2]])
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await reference.loadAll()
    expect(app.currencies.map((c) => c.code)).toEqual(['USD'])
    expect(app.accounts.map((a) => a.id)).toEqual(['acc-1'])
    expect(app.categories.map((c) => c.id)).toEqual(['cat-income'])
    expect(app.currencyMap.get('CNY')).toBeUndefined()
    expect(reference.currencyMap.get('USD')?.name).toBe('美元')
  })
})
