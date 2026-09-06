import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, watch } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createTestingPinia } from '@pinia/testing'
import { invoke } from '@tauri-apps/api/core'
import { useTransactionFilter, UNCATEGORIZED_ONLY, CATEGORY_DRILLDOWN_KINDS } from '@/composables/useTransactionFilter'
import type { UseTransactionFilterReturn } from '@/composables/useTransactionFilter'
import { useReferenceStore } from '@/stores/reference'
import type { Account, Category, Merchant, TransactionListFilter } from '@/types'

const mockInvoke = vi.mocked(invoke)

/** URL 下钻用参考数据：两账户；商户含一软删、分类含一软删（历史交易口径，issue #191/#377 校验含软删）。 */
const urlAccounts: Account[] = [
  {
    id: 'acc-1', name: '现金', type: 'cash', currency_code: 'CNY', initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false, is_hidden: false,
  },
  {
    id: 'acc-2', name: '银行', type: 'bank', currency_code: 'CNY', initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false, is_hidden: false,
  },
]
const urlMerchants: Merchant[] = [
  {
    id: 'mch-1', name: '京东',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'mch-2', name: '红旗连锁',
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: true,
  },
]
const urlCategories: Category[] = [
  {
    id: 'cat-1', name: '餐饮', kind: 'expense', parent_id: null, icon: null, sort_order: 0,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'cat-2', name: '下钻专线', kind: 'expense', parent_id: null, icon: null, sort_order: 1,
    created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: true,
  },
]

beforeEach(() => {
  // Reference Data store 用真实动作（createTestingPinia stubActions:false，ADR-0030 决策 7）：
  // 模块内部消费 store（#234），就绪补判走真实 status 时序，数据由 invoke mock 提供。
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve(urlAccounts)
    if (cmd === 'list_categories') return Promise.resolve(urlCategories)
    if (cmd === 'list_merchants') return Promise.resolve(urlMerchants)
    if (cmd === 'list_insurers') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
})

/** 消费契约镜像（ADR-0030 决策 6）：模块只产出状态与版本信号，请求发起归调用方——
 * 调用方监听 refreshVersion，bump 即以当前模块状态装配请求参数。记录每次实际发生的
 * 请求，供「意图 → 状态终态与请求参数」断言；同步多次 bump 被 watcher 去重为一次请求。 */
let harness: { tf: UseTransactionFilterReturn; requests: TransactionListFilter[] } | null = null

const FilterHarness = defineComponent({
  setup() {
    const tf = useTransactionFilter()
    const requests: TransactionListFilter[] = []
    watch(tf.refreshVersion, () => {
      const f: TransactionListFilter = { page: tf.page.value, page_size: tf.pageSize.value }
      if (tf.filters.dateFrom) f.from = tf.filters.dateFrom
      if (tf.filters.dateTo) f.to = tf.filters.dateTo
      if (tf.filters.involvingAccountId) f.involving_account_id = tf.filters.involvingAccountId
      if (tf.filters.merchantId) f.merchant_id = tf.filters.merchantId
      // 分类维度三态装配（issue #377，与视图 load 同构）：哨兵 → 仅无分类，其余非空值 → 精确 id
      if (tf.filters.categoryId === UNCATEGORIZED_ONLY) f.uncategorized_only = true
      else if (tf.filters.categoryId) f.category_id = tf.filters.categoryId
      if (tf.filters.kind) f.kind = tf.filters.kind
      // 类型集合维度（issue #581，与视图 load 同构）：非空集合 → kinds 数组（浅拷贝脱只读）
      if (tf.filters.kinds?.length) f.kinds = [...tf.filters.kinds]
      requests.push(f)
    })
    harness = { tf, requests }
    return () => null
  },
})

function mountHarness() {
  mount(FilterHarness, {
    global: { plugins: [createTestingPinia({ stubActions: false })] },
  })
  return harness!
}

function lastRequest(): TransactionListFilter {
  const requests = harness!.requests
  expect(requests.length, '应已发生至少一次重拉').toBeGreaterThan(0)
  return requests[requests.length - 1]
}

describe('useTransactionFilter 初始状态', () => {
  it('默认全量：六个过滤维度为 null，page=1，pageSize=20，版本号 0', () => {
    const { tf } = mountHarness()
    expect(tf.filters).toEqual({
      dateFrom: null,
      dateTo: null,
      involvingAccountId: null,
      merchantId: null,
      categoryId: null,
      kind: null,
      kinds: null,
    })
    expect(tf.page.value).toBe(1)
    expect(tf.pageSize.value).toBe(20)
    expect(tf.refreshVersion.value).toBe(0)
  })
})

describe('useTransactionFilter setFilter（手动过滤意图）', () => {
  it('单维度意图：状态终态生效 + 翻页归零 + 一次重拉（请求参数含该维度）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 3 // 已翻页背景下声明意图 → 必须翻回第 1 页
    tf.setFilter({ kind: 'income' })
    await flushPromises()
    expect(tf.filters.kind).toBe('income')
    expect(tf.page.value).toBe(1)
    expect(tf.refreshVersion.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, kind: 'income' })
  })

  it('同值意图不动作：条件实际变化才触发出口（不重拉、不归零）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ kind: 'income' })
    await flushPromises()
    expect(requests).toHaveLength(1)
    // 再次声明同值意图 → 无变化即无出口
    tf.setFilter({ kind: 'income' })
    await flushPromises()
    expect(requests).toHaveLength(1)
    expect(tf.refreshVersion.value).toBe(1)
    expect(tf.page.value).toBe(1)
  })

  it('部分补丁合并：未提及的维度不受牵连', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01' })
    tf.setFilter({ kind: 'expense' })
    await flushPromises()
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kind).toBe('expense')
    expect(tf.filters.dateTo).toBeNull()
    expect(tf.filters.involvingAccountId).toBeNull()
  })

  it('多键补丁一次出口：一次翻页归零 + 一次重拉（中间态不产生多余请求）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 2
    tf.setFilter({ dateFrom: '2026-01-01', dateTo: '2026-01-31', kind: 'income' })
    await flushPromises()
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      from: '2026-01-01',
      to: '2026-01-31',
      kind: 'income',
    })
  })

  it('多条件组合逐维声明：每次变化各走一次出口，最终请求参数完整', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1' })
    tf.setFilter({ dateFrom: '2026-01-01' })
    tf.setFilter({ dateTo: '2026-03-31' })
    tf.setFilter({ kind: 'transfer' })
    tf.setFilter({ merchantId: 'mch-1' })
    tf.setFilter({ categoryId: 'cat-1' })
    await flushPromises()
    expect(tf.filters).toEqual({
      dateFrom: '2026-01-01',
      dateTo: '2026-03-31',
      involvingAccountId: 'acc-1',
      merchantId: 'mch-1',
      categoryId: 'cat-1',
      kind: 'transfer',
      kinds: null,
    })
    // 同一同步批次内的多次 bump 被 watcher 去重，最终以完整过滤状态重拉一次
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      from: '2026-01-01',
      to: '2026-03-31',
      involving_account_id: 'acc-1',
      merchant_id: 'mch-1',
      category_id: 'cat-1',
      kind: 'transfer',
    })
  })
})

describe('useTransactionFilter resetFilters（清除筛选）', () => {
  it('有激活条件：全部维度回默认态 + 翻页归零 + 一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1', kind: 'transfer' })
    await flushPromises()
    tf.page.value = 2
    tf.resetFilters()
    await flushPromises()
    expect(tf.filters).toEqual({
      dateFrom: null,
      dateTo: null,
      involvingAccountId: null,
      merchantId: null,
      categoryId: null,
      kind: null,
      kinds: null,
    })
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(2) // setFilter 一次 + resetFilters 一次
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('无激活条件：幂等不动作（不重拉）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.resetFilters()
    await flushPromises()
    expect(requests).toHaveLength(0)
    expect(tf.refreshVersion.value).toBe(0)
  })
})

describe('useTransactionFilter refresh（外部数据变化回填）', () => {
  it('翻回第一页重拉，不动筛选', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1' })
    tf.page.value = 3
    tf.refresh()
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.page.value).toBe(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, involving_account_id: 'acc-1' })
  })

  it('已在第 1 页仍重拉（记一笔/退款回填可见性）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.refresh()
    await flushPromises()
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })
})

describe('useTransactionFilter 分页所有权', () => {
  it('页大小切换经 refresh 出口：归零 + 以新页大小重拉，过滤条件保持', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1' })
    await flushPromises()
    tf.pageSize.value = 50
    tf.refresh()
    await flushPromises()
    // 过滤维度归模块状态所有，页大小切换不触碰 → 请求同时携带新页大小与既有过滤
    expect(lastRequest()).toEqual({ page: 1, page_size: 50, involving_account_id: 'acc-1' })
  })

  it('翻页导航由调用方直写页码：版本号不 bump、过滤状态不被触碰（视图自行以新页码重拉）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1', kind: 'income' })
    await flushPromises()
    const versionAfterFilter = tf.refreshVersion.value
    const requestsAfterFilter = requests.length
    tf.page.value = 2
    await flushPromises()
    expect(tf.page.value).toBe(2)
    // 组合不变量：翻页直写只动页码，过滤状态原样保留，调用方重拉即同时携带两者
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.filters.kind).toBe('income')
    expect(tf.refreshVersion.value).toBe(versionAfterFilter)
    expect(requests.length).toBe(requestsAfterFilter)
  })
})

// —— 页码回退入口（ADR-0045，删除路径）：声明「删除当前页一行后本页剩 N 条」——
// N 为 0 且非第一页时减一页，然后一律版本 bump；不走 refresh 的「翻回第一页」语义。

describe('useTransactionFilter 页码回退入口（删除路径）', () => {
  it('本页删后剩 0 条且非第一页：回退一页 + 一次重拉（请求以回退后页码与既有筛选发起）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1' })
    await flushPromises()
    tf.page.value = 3
    tf.afterRowDelete(0)
    await flushPromises()
    expect(tf.page.value).toBe(2)
    expect(requests).toHaveLength(2) // setFilter 一次 + 回退入口一次
    expect(lastRequest()).toEqual({ page: 2, page_size: 20, involving_account_id: 'acc-1' })
  })

  it('本页删后剩 ≥1 条：页码不变 + 一次重拉（不回退、也不翻回第一页）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 3
    tf.afterRowDelete(19)
    await flushPromises()
    expect(tf.page.value).toBe(3)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 3, page_size: 20 })
  })

  it('第一页删除（page=1 且剩 0 条）：页码保持 1 + 一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.afterRowDelete(0)
    await flushPromises()
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })
})

describe('useTransactionFilter 工厂形态', () => {
  it('每次调用返回独立实例：状态与版本号互不串扰', async () => {
    const { tf: tf1 } = mountHarness()
    const tf2 = useTransactionFilter()
    await flushPromises()
    tf1.setFilter({ kind: 'income' })
    await flushPromises()
    expect(tf1.filters.kind).toBe('income')
    expect(tf1.refreshVersion.value).toBe(1)
    expect(tf2.filters.kind).toBeNull()
    expect(tf2.refreshVersion.value).toBe(0)
    expect(tf2.page.value).toBe(1)
  })
})

// —— URL 下钻参数表（issue #234）：解析、校验、复位规则、就绪补判与字段级让位内化于模块 ——
// 视图仅把 route query 变化递给模块（syncUrlQuery）；以下用例打模块接口，
// query 以普通对象递入（与 vue-router LocationQuery 结构兼容，非字符串值视为不在场）。

describe('useTransactionFilter URL 参数表·解析与校验（参考数据已就绪）', () => {
  it('账户直达：有效 account 参数立即校验应用，走统一出口（翻页归零 + 一次重拉）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 3 // 已翻页背景下下钻 → 必须翻回第 1 页
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, involving_account_id: 'acc-1' })
  })

  it('商户直达：在用与软删商户均有效（历史交易口径，issue #191）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'mch-2' }) // mch-2 为软删商户
    await flushPromises()
    expect(tf.filters.merchantId).toBe('mch-2')
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, merchant_id: 'mch-2' })
  })

  it('组合直达：account + merchant 同时生效，一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1', merchant: 'mch-1' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.filters.merchantId).toBe('mch-1')
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
      merchant_id: 'mch-1',
    })
  })

  it('无效参数回退：校验失败维度清空；两维度均无有效参数时复位日期/类型（#96 决策 3）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    tf.syncUrlQuery({ account: 'missing-acc' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.kind).toBeNull()
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('无效参数回退·merchant 维度同规则：字典中不存在的商户同样回退并复位', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'missing-mch' })
    await flushPromises()
    expect(tf.filters.merchantId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.kind).toBeNull()
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('不带参数进入：全量默认态，不产生出口', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({})
    await flushPromises()
    expect(tf.filters).toEqual({
      dateFrom: null,
      dateTo: null,
      involvingAccountId: null,
      merchantId: null,
      categoryId: null,
      kind: null,
      kinds: null,
    })
    expect(requests).toHaveLength(0)
  })

  it('非字符串参数视为不在场：数组值不应用也不产生出口', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: ['acc-1'] })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(requests).toHaveLength(0)
  })

  it('参数未变化不重放：同值再递（无关导航）不产生出口、不覆盖手动改动', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01' })
    await flushPromises()
    // 无关导航替换 query 对象，account 值未变 → 该维度不动作
    tf.syncUrlQuery({ account: 'acc-1', unrelated: 'x' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(requests).toHaveLength(2)
  })

  it('导航换参：account 参数 a → b 按新参数重新消费', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-2' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-2')
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, involving_account_id: 'acc-2' })
  })
})

describe('useTransactionFilter URL 参数表·复位规则（#96 决策 3）', () => {
  it('导航清除参数：对应维度同步清空 + 日期/类型复位 + 翻页归零', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01' })
    tf.page.value = 2
    await flushPromises()
    tf.syncUrlQuery({})
    await flushPromises()
    expect(tf.filters).toEqual({
      dateFrom: null,
      dateTo: null,
      involvingAccountId: null,
      merchantId: null,
      categoryId: null,
      kind: null,
      kinds: null,
    })
    expect(tf.page.value).toBe(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('另一维度参数在场：被清除维度清空，日期/类型不越界复位', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1', merchant: 'mch-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    // 导航清除 account 参数（merchant 仍在场）
    tf.syncUrlQuery({ merchant: 'mch-1' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.merchantId).toBe('mch-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kind).toBe('income')
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      merchant_id: 'mch-1',
      from: '2026-01-01',
      kind: 'income',
    })
  })

  it('导航清除 merchant 参数：对应维度同步清空 + 日期/类型复位 + 翻页归零', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'mch-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01' })
    tf.page.value = 2
    await flushPromises()
    tf.syncUrlQuery({})
    await flushPromises()
    expect(tf.filters.merchantId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.page.value).toBe(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('复位守卫对称：merchant 在场、account 无效进入时账户维度清空，日期/类型不越界复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'mch-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'mch-1', account: 'missing-acc' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.merchantId).toBe('mch-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kind).toBe('income')
  })
})

// —— 分类维度（issue #377）：URL ?category= 下钻，合法 id 精确过滤、保留值表示仅无分类、
// 非法/未知回退不过滤；校验映射含软删分类（历史交易口径，先例商户）；
// 挂起补判/让位/导航清除与账户/商户维度同规则。

describe('useTransactionFilter URL 参数表·分类维度（issue #377）', () => {
  it('合法分类 id：精确过滤应用（请求携带 category_id，翻页归零 + 一次重拉）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 3
    tf.syncUrlQuery({ category: 'cat-1' })
    await flushPromises()
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, category_id: 'cat-1' })
  })

  it('保留值 none：仅无分类（哨兵态，请求携带 uncategorized_only: true）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: UNCATEGORIZED_ONLY })
    await flushPromises()
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, uncategorized_only: true })
  })

  it('软删分类 id 有效（历史交易口径）：校验应用不回退', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-2' }) // cat-2 为软删分类
    await flushPromises()
    expect(tf.filters.categoryId).toBe('cat-2')
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, category_id: 'cat-2' })
  })

  it('未知分类 id：回退不过滤；另一维度（账户）有效在场时不误清其他维度、不越界复位', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1', category: 'missing-cat' })
    await flushPromises()
    expect(tf.filters.categoryId).toBeNull()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kind).toBe('income')
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
      from: '2026-01-01',
      kind: 'income',
    })
  })

  it('组合直达：account + category 同时生效，一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1', category: 'cat-1' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
      category_id: 'cat-1',
    })
  })

  it('导航清除 category 参数：对应维度同步清空 + 日期/类型复位 + 翻页归零', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01' })
    tf.page.value = 2
    await flushPromises()
    tf.syncUrlQuery({})
    await flushPromises()
    expect(tf.filters.categoryId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.page.value).toBe(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('复位守卫对称：category=none 有效在场时 account 无效进入，账户维度清空但日期/类型不越界复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: UNCATEGORIZED_ONLY })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    tf.syncUrlQuery({ category: UNCATEGORIZED_ONLY, account: 'missing-acc' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kind).toBe('income')
  })

  it('导航换参：category 参数 id → 保留值，按新参数重新消费', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1' })
    await flushPromises()
    tf.syncUrlQuery({ category: UNCATEGORIZED_ONLY })
    await flushPromises()
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, uncategorized_only: true })
  })

  it('有效分类参数挂起待就绪，就绪后补判应用；保留值同规则挂起', async () => {
    const release = gateReference('list_categories')
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: UNCATEGORIZED_ONLY, account: 'acc-1' })
    await flushPromises()
    // 分类表未就绪 → status 未 ready → 全部维度统一挂起（不误判为无效）
    expect(tf.filters.categoryId).toBeNull()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(requests).toHaveLength(0)
    release()
    await flushPromises()
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
      uncategorized_only: true,
    })
  })

  it('补判前手动改动同维度（无手动控件，setFilter 直写即手动意图）→ 让位且不再重放', async () => {
    const release = gateReference('list_categories')
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1' })
    await flushPromises()
    // 参考数据就绪前，用户手动改动分类维度（与 URL 参数同维度）
    tf.setFilter({ categoryId: UNCATEGORIZED_ONLY })
    release()
    await flushPromises()
    // 分类维度让位：保持手动改动；之后参考数据重拉不重放
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
    await useReferenceStore().refresh()
    await flushPromises()
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
  })
})

describe('useTransactionFilter URL 参数表·日期维度（issue #380）', () => {
  it('合法 dateFrom/dateTo：应用（请求携带 from/to），翻页归零 + 一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 3
    tf.syncUrlQuery({ dateFrom: '2026-01-01', dateTo: '2026-12-31' })
    await flushPromises()
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.dateTo).toBe('2026-12-31')
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, from: '2026-01-01', to: '2026-12-31' })
  })

  it('报表跳转载荷形态：category + 当年首尾日期 + 类型集合组合直达，一次重拉，单值类型不受牵连', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({
      category: 'cat-1',
      dateFrom: '2026-01-01',
      dateTo: '2026-12-31',
      kinds: CATEGORY_DRILLDOWN_KINDS,
    })
    await flushPromises()
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.dateTo).toBe('2026-12-31')
    expect(tf.filters.kinds).toEqual(['expense', 'refund'])
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      category_id: 'cat-1',
      from: '2026-01-01',
      to: '2026-12-31',
      kinds: ['expense', 'refund'],
    })
    // 类型集合维度与单值手动类型维度解耦：URL 载荷不触碰单值 kind
    expect(lastRequest().kind).toBeUndefined()
  })

  it('非法格式日期回退不过滤（参数视为不在场），不误清其他维度', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', dateFrom: 'banana', dateTo: '2026-13-99' })
    await flushPromises()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.dateTo).toBeNull()
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, category_id: 'cat-1' })
  })

  it('复位守卫：分类参数无效回退时，日期参数有效在场 → 日期/类型不越界复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'missing-cat', dateFrom: '2026-01-01', dateTo: '2026-12-31' })
    await flushPromises()
    expect(tf.filters.categoryId).toBeNull()
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.dateTo).toBe('2026-12-31')
  })

  it('导航清除日期参数：对应维度同步清空（分类参数在场时不清分类维度）', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', dateFrom: '2026-01-01', dateTo: '2026-12-31' })
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1' })
    await flushPromises()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.dateTo).toBeNull()
    expect(tf.filters.categoryId).toBe('cat-1')
  })

  it('日期参数与分类参数统一挂起待就绪，就绪后一次性补判应用', async () => {
    const release = gateReference('list_categories')
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', dateFrom: '2026-01-01', dateTo: '2026-12-31' })
    await flushPromises()
    // 参考数据未就绪 → 全部维度统一挂起（同批次应用，列表只刷一次）
    expect(tf.filters.categoryId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(requests).toHaveLength(0)
    release()
    await flushPromises()
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.dateTo).toBe('2026-12-31')
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      category_id: 'cat-1',
      from: '2026-01-01',
      to: '2026-12-31',
    })
  })

  it('补判前手动改动日期维度 → 日期参数让位（分类维度补判不受牵连）', async () => {
    const release = gateReference('list_categories')
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', dateFrom: '2026-01-01', dateTo: '2026-12-31' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2025-06-01', dateTo: '2025-06-30' })
    release()
    await flushPromises()
    // 日期让位：保持手动改动；分类维度照常补判应用
    expect(tf.filters.dateFrom).toBe('2025-06-01')
    expect(tf.filters.dateTo).toBe('2025-06-30')
    expect(tf.filters.categoryId).toBe('cat-1')
  })

  it('无效日期回退触发复位契约：无其他有效参数时复位日期/类型（#96 决策 3 语义不变）', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    // 导航清除 account 且 dateFrom 变为非法：全部下钻参数均无效 → 复位日期/类型
    tf.syncUrlQuery({ dateFrom: 'not-a-date' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.kind).toBeNull()
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })
})

// —— 类型集合维度（issue #581）：URL ?kinds= 下钻专用，逗号分隔闭集字面量；无参考数据
// 映射、不涉保留值，挂起补判/让位/复位守卫与既有维度同规。消费方是报表分类下钻跳转，
// 与「仅无分类」解耦：仅无分类命中一切无分类交易、不限定类型。

describe('useTransactionFilter URL 参数表·类型集合维度（issue #581）', () => {
  it('合法集合：应用（请求携带 kinds 数组），翻页归零 + 一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 3
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['expense', 'refund'])
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      kinds: ['expense', 'refund'],
    })
  })

  it('未分类柱下钻形态：kinds × category=none × 期间三维度组合，一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({
      category: UNCATEGORIZED_ONLY,
      dateFrom: '2026-01-01',
      dateTo: '2026-12-31',
      kinds: CATEGORY_DRILLDOWN_KINDS,
    })
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['expense', 'refund'])
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      uncategorized_only: true,
      from: '2026-01-01',
      to: '2026-12-31',
      kinds: ['expense', 'refund'],
    })
  })

  it('非法字面量：整串视为不在场（回退不过滤），不误清其他维度、不越界复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', kinds: 'expense,bogus' })
    await flushPromises()
    expect(tf.filters.kinds).toBeNull()
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, category_id: 'cat-1' })
  })

  it('复位守卫：kinds 无效回退时另一维度有效在场 → 日期/类型不越界复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', kinds: 'expense,bogus' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kind: 'income' })
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', kinds: 'transfer,bogus' })
    await flushPromises()
    expect(tf.filters.kinds).toBeNull()
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kind).toBe('income')
  })

  it('导航清除 kinds 参数：对应维度同步清空（分类参数在场时不清分类维度）', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', kinds: 'expense,refund' })
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1' })
    await flushPromises()
    expect(tf.filters.kinds).toBeNull()
    expect(tf.filters.categoryId).toBe('cat-1')
  })

  it('参考数据未就绪时同规挂起，就绪后补判应用', async () => {
    const release = gateReference('list_accounts')
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    expect(tf.filters.kinds).toBeNull()
    expect(requests).toHaveLength(0)
    release()
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['expense', 'refund'])
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      kinds: ['expense', 'refund'],
    })
  })

  it('单值手动类型维度与类型集合维度 AND 共存，互不改写', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    tf.setFilter({ kind: 'income' })
    await flushPromises()
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      kind: 'income',
      kinds: ['expense', 'refund'],
    })
  })

  it('resetFilters（清除筛选）复位类型集合维度', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    tf.resetFilters()
    await flushPromises()
    expect(tf.filters.kinds).toBeNull()
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('补判前手动改动同维度（setFilter 直写即手动意图）→ 让位且不再重放', async () => {
    const release = gateReference('list_accounts')
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    // 参考数据就绪前，用户手动改动同维度
    tf.setFilter({ kinds: ['transfer'] })
    release()
    await flushPromises()
    // 让位：保持手动改动；之后参考数据重拉不重放
    expect(tf.filters.kinds).toEqual(['transfer'])
    await useReferenceStore().refresh()
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['transfer'])
  })
})

/** 挂起某张参考表响应：模拟冷启动深链时该表晚到；返回放行函数。 */
function gateReference(gatedCmd: 'list_accounts' | 'list_merchants' | 'list_categories') {
  let release!: () => void
  const pending = new Promise<Account[] | Merchant[] | Category[]>((res) => {
    release = () =>
      res(
        gatedCmd === 'list_accounts'
          ? urlAccounts
          : gatedCmd === 'list_merchants'
            ? urlMerchants
            : urlCategories,
      )
  })
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === gatedCmd) return pending
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve(urlAccounts)
    if (cmd === 'list_categories') return Promise.resolve(urlCategories)
    if (cmd === 'list_merchants') return Promise.resolve(urlMerchants)
    if (cmd === 'list_insurers') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  return release
}

describe('useTransactionFilter URL 参数表·就绪补判（模块内部消费 Reference Data store）', () => {
  it('有效参数挂起待就绪，就绪后补判应用（不静默丢失）', async () => {
    const release = gateReference('list_accounts')
    const { tf, requests } = mountHarness()
    await flushPromises()
    expect(useReferenceStore().status).toBe('loading')
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    // 挂起：不误判为无效而回退，也不误应用
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(requests).toHaveLength(0)
    release()
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, involving_account_id: 'acc-1' })
  })

  it('无效参数挂起，就绪后校验失败回退全量（不报错、无谓出口）', async () => {
    const release = gateReference('list_accounts')
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'missing-acc' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    release()
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(requests).toHaveLength(0)
  })

  it('merchant 维度同样挂起待就绪：软删商户参数就绪后补判应用', async () => {
    const release = gateReference('list_merchants')
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'mch-2' })
    await flushPromises()
    expect(tf.filters.merchantId).toBeNull()
    expect(requests).toHaveLength(0)
    release()
    await flushPromises()
    expect(tf.filters.merchantId).toBe('mch-2')
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, merchant_id: 'mch-2' })
  })
})

describe('useTransactionFilter URL 参数表·字段级让位（issue #234 新增行为）', () => {
  it('补判前手动改动同维度 → 让位且不再重放；其他维度补判不受牵连', async () => {
    const release = gateReference('list_accounts')
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1', merchant: 'mch-1' })
    await flushPromises()
    // 参考数据就绪前，用户手动改动账户维度（与 URL 参数同维度）
    tf.setFilter({ involvingAccountId: 'acc-2' })
    release()
    await flushPromises()
    // 账户维度让位：保持手动改动；商户维度照常补判应用
    expect(tf.filters.involvingAccountId).toBe('acc-2')
    expect(tf.filters.merchantId).toBe('mch-1')
    // 之后参考数据重拉（status 再次 ready）不重放已让位的参数
    await useReferenceStore().refresh()
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-2')
    expect(tf.filters.merchantId).toBe('mch-1')
  })

  it('补判前 resetFilters（显式清空全部维度）→ 挂起参数让位', async () => {
    const release = gateReference('list_accounts')
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ kind: 'income' }) // 制造激活条件，使 resetFilters 实际动作
    tf.resetFilters()
    release()
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.kind).toBeNull()
  })

  it('参考数据重拉不重放：已结算参数在 status 再次 ready 后不覆盖手动改动', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-2' })
    await useReferenceStore().refresh()
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBe('acc-2')
  })
})
