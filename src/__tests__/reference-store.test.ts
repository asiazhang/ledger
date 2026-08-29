import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore, REFERENCE_FRESH_MS } from '@/stores/reference'
import type { Account, Category, Currency, Merchant } from '@/types'

const mockInvoke = vi.mocked(invoke)
const mockListen = vi.mocked(listen)

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

const mockMerchants: Merchant[] = [
  {
    id: 'mch-1', name: '京东',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'mch-2', name: '红旗连锁',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

/** 重拉/事件后的新数据（用于验证 stale-while-revalidate 的替换）。 */
const newCurrencies: Currency[] = [
  { code: 'EUR', name: '欧元', symbol: '€', decimal_places: 2 },
]
const newAccounts: Account[] = [
  {
    id: 'acc-new', name: '新账户', type: 'bank', currency_code: 'USD',
    initial_balance_cents: 0, created_at: '2026-02-01T00:00:00Z',
    updated_at: '2026-02-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false, is_hidden: false,
  },
]
const newCategories: Category[] = [
  {
    id: 'cat-new', name: '新分类', kind: 'expense', parent_id: null,
    icon: null, sort_order: 0, created_at: '2026-02-01T00:00:00Z',
    updated_at: '2026-02-01T00:00:00Z', version: 1, device_id: 'test',
    is_deleted: false,
  },
]
const newMerchants: Merchant[] = [
  {
    created_at: '2026-02-01T00:00:00Z', updated_at: '2026-02-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
]

function mockListCommands() {
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
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
    expect(store.merchants).toEqual([])
  })

  it('refresh 拉取四张参考表并填充响应式状态', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)
    expect(store.merchants).toEqual(mockMerchants)
  })

  it('派生映射 currencyMap/accountMap/categoryMap 正确', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.currencyMap.get('USD')?.name).toBe('美元')
    expect(store.accountMap.get('acc-2')?.name).toBe('招商银行')
    expect(store.categoryMap.get('cat-child')?.name).toBe('外卖')
  })

  it('商户派生映射 merchantMap（含按名字查找 merchantByName）正确', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.merchantMap.get('mch-1')?.name).toBe('京东')
    expect(store.merchantByName.get('红旗连锁')?.id).toBe('mch-2')
  })

  it('list_merchants 以含软删全量拉取（includeDeleted=true，筛选下拉数据源 issue #191）', async () => {
    const store = useReferenceStore()
    await store.refresh()
    const merchantCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_merchants')
    expect(merchantCalls.length).toBeGreaterThan(0)
    for (const [, args] of merchantCalls) {
      expect(args).toMatchObject({ includeDeleted: true })
    }
  })

  it('软删商户：从字典与选择列表消失（不可再选），merchantMap 仍保留（历史引用照常显示）', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.merchantMap.get('mch-1')?.name).toBe('京东')

    // 京东被软删：后端含软删列表返回 is_deleted=true 行，其余表不变
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve(mockCategories)
      if (cmd === 'list_merchants') {
        return Promise.resolve([{ ...mockMerchants[0], is_deleted: true }, mockMerchants[1]])
      }
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await store.refresh()

    // 选择列表（merchants / merchantByName）不含软删商户
    expect(store.merchants.map((m) => m.id)).toEqual(['mch-2'])
    expect(store.merchantByName.get('京东')).toBeUndefined()
    // 显示映射仍保留软删商户（历史交易照常显示商户名）
    expect(store.merchantMap.get('mch-1')?.name).toBe('京东')
    expect(store.merchantMap.get('mch-2')?.name).toBe('红旗连锁')
  })

  it('分类树派生：rootCategories/expenseCategories/incomeCategories', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.rootCategories.map((c) => c.id)).toEqual(['cat-root', 'cat-income'])
    expect(store.expenseCategories.map((c) => c.id)).toEqual(['cat-root', 'cat-child'])
    expect(store.incomeCategories.map((c) => c.id)).toEqual(['cat-income'])
  })

  it('categoryChildren/categoryPath/treeCategoryOptions 正确', async () => {
    const store = useReferenceStore()
    await store.refresh()
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
    await store.refresh()
    expect(store.getCurrency('CNY')?.symbol).toBe('¥')
    expect(store.getCurrency('EUR')).toBeUndefined()
  })
})

describe('useReferenceStore 失效信号与 push 生命周期', () => {
  let changedHandler: ((...args: unknown[]) => void) | null = null

  beforeEach(() => {
    mockListen.mockReset()
    mockListen.mockImplementation((_event: string, handler: (...args: unknown[]) => void) => {
      changedHandler = handler
      return Promise.resolve(vi.fn())
    })
  })

  afterEach(() => {
    changedHandler = null
    vi.useRealTimers()
  })

  it('首次访问 self-init 自动触发一次加载（无需手动调用 load*）', async () => {
    const store = useReferenceStore()
    // self-init 同步发起了三张参考表的拉取（恰一次）
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd.startsWith('list_')),
    ).toHaveLength(4)
    await store.ensureFresh()
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)
  })

  it('status/version 迁移：self-init → loading → ready，成功重拉 version 自增', async () => {
    const store = useReferenceStore()
    expect(store.status).toBe('loading') // self-init 同步置位
    expect(store.version).toBe(0)
    await store.ensureFresh()
    expect(store.status).toBe('ready')
    expect(store.version).toBe(1)
    await store.refresh()
    expect(store.status).toBe('ready')
    expect(store.version).toBe(2)
  })

  it('listen 在 store 首次访问时注册一次（订阅 ledger:changed）', () => {
    useReferenceStore()
    // pinia store 为单例：再次访问不重复注册
    useReferenceStore()
    expect(mockListen).toHaveBeenCalledTimes(1)
    expect(mockListen).toHaveBeenCalledWith('ledger:changed', expect.any(Function))
  })

  it('ledger:changed 到达后置 loading 并保留旧数据（不闪空），完成后替换', async () => {
    const store = useReferenceStore()
    await store.ensureFresh()

    let resolveCats!: (v: Category[]) => void
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(newCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(newAccounts)
      if (cmd === 'list_categories') {
        return new Promise((res) => {
          resolveCats = res
        })
      }
      if (cmd === 'list_merchants') return Promise.resolve(newMerchants)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    changedHandler?.({ payload: undefined })
    // 事件到达即置 loading，旧数据保留（stale-while-revalidate）
    expect(store.status).toBe('loading')
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)

    resolveCats(newCategories)
    await flushPromises()
    expect(store.status).toBe('ready')
    expect(store.currencies).toEqual(newCurrencies)
    expect(store.accounts).toEqual(newAccounts)
    expect(store.categories).toEqual(newCategories)
    expect(store.version).toBe(2)
  })

  it('触发 ledger:changed 后四表自动更新，派生映射随之更新', async () => {
    const store = useReferenceStore()
    await store.ensureFresh()
    expect(store.currencyMap.get('CNY')?.name).toBe('人民币')

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(newCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(newAccounts)
      if (cmd === 'list_categories') return Promise.resolve(newCategories)
      if (cmd === 'list_merchants') return Promise.resolve(newMerchants)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    changedHandler?.({ payload: undefined })
    await flushPromises()

    expect(store.currencies).toEqual(newCurrencies)
    expect(store.accounts).toEqual(newAccounts)
    expect(store.categories).toEqual(newCategories)
    // 派生映射（computed）自动更新
    expect(store.currencyMap.get('EUR')?.name).toBe('欧元')
    expect(store.currencyMap.get('CNY')).toBeUndefined()
    expect(store.accountMap.get('acc-new')?.name).toBe('新账户')
    expect(store.categoryMap.get('cat-new')?.name).toBe('新分类')
    expect(store.rootCategories.map((c) => c.id)).toEqual(['cat-new'])
    expect(store.version).toBe(2)
  })

  it('ensureFresh 在 fresh 窗口内零 IPC；refresh 强制绕过窗口重拉', async () => {
    const store = useReferenceStore()
    await store.ensureFresh()
    mockInvoke.mockClear()

    await store.ensureFresh()
    expect(mockInvoke).not.toHaveBeenCalled()

    await store.refresh()
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd.startsWith('list_')),
    ).toHaveLength(4)
  })

  it('并发 ensureFresh 合并为一次 IPC', async () => {
    vi.useFakeTimers()
    const store = useReferenceStore()
    await store.ensureFresh()
    mockInvoke.mockClear()
    // 越过新鲜度窗口，使并发调用真正走重拉
    vi.advanceTimersByTime(REFERENCE_FRESH_MS + 1)

    let resolveCats!: (v: Category[]) => void
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(newCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(newAccounts)
      if (cmd === 'list_categories') {
        return new Promise((res) => {
          resolveCats = res
        })
      }
      if (cmd === 'list_merchants') return Promise.resolve(newMerchants)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })

    const p1 = store.ensureFresh()
    const p2 = store.ensureFresh()
    const p3 = store.ensureFresh()
    // 三次并发调用只发起一次加载（三张表各一次 IPC）
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd.startsWith('list_')),
    ).toHaveLength(4)
    resolveCats(newCategories)
    await Promise.all([p1, p2, p3])
    expect(store.categories).toEqual(newCategories)
    expect(store.version).toBe(2)
  })

  it('重拉不闪空：加载期间保留旧数据，成功后才整体替换', async () => {
    const store = useReferenceStore()
    await store.ensureFresh()

    let resolveCats!: (v: Category[]) => void
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(newCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(newAccounts)
      if (cmd === 'list_categories') {
        return new Promise((res) => {
          resolveCats = res
        })
      }
      if (cmd === 'list_merchants') return Promise.resolve(newMerchants)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })

    const p = store.refresh()
    // 重拉期间：status=loading，旧数据原样保留（不闪空）
    expect(store.status).toBe('loading')
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)

    resolveCats(newCategories)
    await p
    expect(store.status).toBe('ready')
    expect(store.currencies).toEqual(newCurrencies)
    expect(store.accounts).toEqual(newAccounts)
    expect(store.categories).toEqual(newCategories)
    expect(store.version).toBe(2)
  })

  it('重拉失败 → status=error、保留旧数据、version 不变', async () => {
    const store = useReferenceStore()
    await store.ensureFresh()

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.reject(new Error('db 错误'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await expect(store.refresh()).rejects.toThrow('db 错误')
    expect(store.status).toBe('error')
    expect(store.version).toBe(1)
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)
  })

  it('失败后 refresh 可恢复：error → loading → ready，version 续增', async () => {
    const store = useReferenceStore()
    await store.ensureFresh()

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.reject(new Error('db 错误'))
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await expect(store.refresh()).rejects.toThrow('db 错误')
    expect(store.status).toBe('error')

    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(newCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(newAccounts)
      if (cmd === 'list_categories') return Promise.resolve(newCategories)
      if (cmd === 'list_merchants') return Promise.resolve(newMerchants)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    await store.refresh()
    expect(store.status).toBe('ready')
    expect(store.version).toBe(2)
    expect(store.currencies).toEqual(newCurrencies)
  })
})
