import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Account, Category, Currency, Insurer, Merchant } from '@/types'

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

const mockInsurers: Insurer[] = [
  {
    id: 'ins-1', name: '平安人寿',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'ins-del', name: '已删保司',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: true,
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

// 参考命令桩统一走共享助手（issue #725）；本文件以自身夹具为被测数据，全量覆写。
function mockListCommands() {
  stubReferenceInvoke({
    list_currencies: mockCurrencies,
    list_accounts: mockAccounts,
    list_categories: mockCategories,
    list_merchants: mockMerchants,
    list_insurers: mockInsurers,
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
    expect(store.insurers).toEqual([])
    expect(store.deletedInsurers.size).toBe(0)
  })

  it('refresh 拉取五张参考表并填充响应式状态（保司 issue #714）', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)
    expect(store.merchants).toEqual(mockMerchants)
    expect(store.insurers.map((i) => i.id)).toEqual(['ins-1'])
    expect(store.deletedInsurers.get('ins-del')?.name).toBe('已删保司')
  })

  it('list_insurers 以含已删全量拉取（includeDeleted=true，管理视图「显示已删」数据源 issue #714）', async () => {
    const store = useReferenceStore()
    await store.refresh()
    const insurerCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_insurers')
    expect(insurerCalls.length).toBeGreaterThan(0)
    for (const [, args] of insurerCalls) {
      expect(args).toMatchObject({ includeDeleted: true })
    }
  })

  it('软删保司：从在用字典消失（不可再选），deletedInsurers 保留（管理视图已删区 issue #714）', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.insurers.map((i) => i.id)).toEqual(['ins-1'])

    // 平安人寿被软删：后端含已删列表返回 is_deleted=true 行
    stubReferenceInvoke({
      list_currencies: mockCurrencies,
      list_accounts: mockAccounts,
      list_categories: mockCategories,
      list_merchants: mockMerchants,
      list_insurers: [{ ...mockInsurers[0], is_deleted: true }, mockInsurers[1]],
    })
    await store.refresh()

    expect(store.insurers).toEqual([])
    expect(store.deletedInsurers.get('ins-1')?.name).toBe('平安人寿')
    expect(store.deletedInsurers.get('ins-del')?.name).toBe('已删保司')
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

  it('保司字典接入（issue #713 / ADR-0082）：在用进字典与按名查找，含已删全量拉取，insurerMap 含软删行', async () => {
    const store = useReferenceStore()
    await store.refresh() // 等 self-init 完成（避免与在途加载合并去重）
    // 保司拉取以含已删全量（同商户先例：在用进字典，软删进显示映射）
    stubReferenceInvoke({
      list_currencies: mockCurrencies,
      list_accounts: mockAccounts,
      list_categories: mockCategories,
      list_insurers: [
        { id: 'ins-1', name: '平安人寿', is_deleted: false, created_at: '', updated_at: '', version: 1, device_id: 'test' },
        { id: 'ins-2', name: '海峡金桥', is_deleted: true, created_at: '', updated_at: '', version: 1, device_id: 'test' },
      ],
    })
    await store.refresh()

    const insurerCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_insurers')
    expect(insurerCalls.length).toBeGreaterThan(0)
    for (const [, args] of insurerCalls) {
      expect(args).toMatchObject({ includeDeleted: true })
    }
    // 在用：进字典、进按名查找；软删：只进显示映射（存量保单保司列照常显示）
    expect(store.insurers.map((i) => i.id)).toEqual(['ins-1'])
    expect(store.insurerByName.get('平安人寿')?.id).toBe('ins-1')
    expect(store.insurerByName.get('海峡金桥')).toBeUndefined()
    expect(store.insurerMap.get('ins-1')?.name).toBe('平安人寿')
    expect(store.insurerMap.get('ins-2')?.name).toBe('海峡金桥')
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
    stubReferenceInvoke({
      list_currencies: mockCurrencies,
      list_accounts: mockAccounts,
      list_categories: mockCategories,
      list_merchants: [{ ...mockMerchants[0], is_deleted: true }, mockMerchants[1]],
      list_insurers: mockInsurers,
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

  it('categoryDisplayName：子分类路径名、顶级自身名，解析不到回退兜底名（issue #356）', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.categoryDisplayName('cat-child', '未分类')).toBe('餐饮 > 外卖')
    expect(store.categoryDisplayName('cat-root', '未分类')).toBe('餐饮')
    // 孤儿引用（分类已删）与空 id：回退调用方提供的后端兜底名，不抛错
    expect(store.categoryDisplayName('cat-gone', '未分类')).toBe('未分类')
    expect(store.categoryDisplayName(null, '未分类')).toBe('未分类')
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
  })

  it('首次访问 self-init 自动触发一次加载（无需手动调用 load*）', async () => {
    const store = useReferenceStore()
    // self-init 同步发起了五张参考表的拉取（恰一次）
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd.startsWith('list_')),
    ).toHaveLength(5)
    await store.refresh()
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)
  })

  it('status/version 迁移：self-init → loading → ready，成功重拉 version 自增', async () => {
    const store = useReferenceStore()
    expect(store.status).toBe('loading') // self-init 同步置位
    expect(store.version).toBe(0)
    await store.refresh()
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
    await store.refresh()

    let resolveCats!: (v: Category[]) => void
    stubReferenceInvoke({
      list_currencies: newCurrencies,
      list_accounts: newAccounts,
      list_categories: () =>
        new Promise((res) => {
          resolveCats = res
        }),
      list_merchants: newMerchants,
      list_insurers: mockInsurers,
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

  it('触发 ledger:changed 后五表自动更新，派生映射随之更新', async () => {
    const store = useReferenceStore()
    await store.refresh()
    expect(store.currencyMap.get('CNY')?.name).toBe('人民币')

    stubReferenceInvoke({
      list_currencies: newCurrencies,
      list_accounts: newAccounts,
      list_categories: newCategories,
      list_merchants: newMerchants,
      list_insurers: mockInsurers,
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

  it('并发 refresh 合并为一次 IPC', async () => {
    const store = useReferenceStore()
    await store.refresh()
    mockInvoke.mockClear()

    let resolveCats!: (v: Category[]) => void
    stubReferenceInvoke({
      list_currencies: newCurrencies,
      list_accounts: newAccounts,
      list_categories: () =>
        new Promise((res) => {
          resolveCats = res
        }),
      list_merchants: newMerchants,
      list_insurers: mockInsurers,
    })

    const p1 = store.refresh()
    const p2 = store.refresh()
    const p3 = store.refresh()
    // 三次并发调用只发起一次加载（五张表各一次 IPC）
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd.startsWith('list_')),
    ).toHaveLength(5)
    resolveCats(newCategories)
    await Promise.all([p1, p2, p3])
    expect(store.categories).toEqual(newCategories)
    expect(store.version).toBe(2)
  })

  it('重拉不闪空：加载期间保留旧数据，成功后才整体替换', async () => {
    const store = useReferenceStore()
    await store.refresh()

    let resolveCats!: (v: Category[]) => void
    stubReferenceInvoke({
      list_currencies: newCurrencies,
      list_accounts: newAccounts,
      list_categories: () =>
        new Promise((res) => {
          resolveCats = res
        }),
      list_merchants: newMerchants,
      list_insurers: mockInsurers,
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
    await store.refresh()

    stubReferenceInvoke({ list_currencies: () => Promise.reject(new Error('db 错误')) })
    await expect(store.refresh()).rejects.toThrow('db 错误')
    expect(store.status).toBe('error')
    expect(store.version).toBe(1)
    expect(store.currencies).toEqual(mockCurrencies)
    expect(store.accounts).toEqual(mockAccounts)
    expect(store.categories).toEqual(mockCategories)
  })

  it('失败后 refresh 可恢复：error → loading → ready，version 续增', async () => {
    const store = useReferenceStore()
    await store.refresh()

    stubReferenceInvoke({ list_currencies: () => Promise.reject(new Error('db 错误')) })
    await expect(store.refresh()).rejects.toThrow('db 错误')
    expect(store.status).toBe('error')

    stubReferenceInvoke({
      list_currencies: newCurrencies,
      list_accounts: newAccounts,
      list_categories: newCategories,
      list_merchants: newMerchants,
      list_insurers: mockInsurers,
    })
    await store.refresh()
    expect(store.status).toBe('ready')
    expect(store.version).toBe(2)
    expect(store.currencies).toEqual(newCurrencies)
  })
})
