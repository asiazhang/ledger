import { beforeEach, describe, expect, it } from 'vitest'
import { wireInvokeSeam } from './helpers/invoke-mock'
import { defineComponent, watch } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createTestingPinia } from '@pinia/testing'
import { useTransactionFilter, UNCATEGORIZED_ONLY, CATEGORY_DRILLDOWN_KINDS } from '@/composables/useTransactionFilter'
import type { UseTransactionFilterReturn } from '@/composables/useTransactionFilter'
import { useReferenceStore } from '@/stores/reference'
import type { Account, Category, Merchant, TransactionKind, TransactionListFilter } from '@/types'
import { TRANSACTION_KINDS } from '@/types'


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
    updated_at: '2026-01-01T00:00:00Z',
    version: 1, device_id: 'test', is_deleted: false,
  },
  {
    id: 'mch-2', name: '红旗连锁',
    updated_at: '2026-01-01T00:00:00Z',
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
  // URL 下钻用参考数据（issue #725 共享助手）：只覆写本模块行使的三张表
  wireInvokeSeam({
    overrides: {
      list_accounts: urlAccounts,
      list_categories: urlCategories,
      list_merchants: urlMerchants,
    },
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
      // 类型维度（手动多选 + 下钻共用，spec #1025，与视图 load 同构）：非空集合 → kinds 数组（浅拷贝脱只读）
      if (tf.filters.kinds?.length) f.kinds = [...tf.filters.kinds]
      requests.push(f)
    })
    harness = { tf, requests }
    return () => null
  },
})

function mountHarness() {
  const wrapper = mount(FilterHarness, {
    global: { plugins: [createTestingPinia({ stubActions: false })] },
  })
  return { wrapper, ...harness! }
}

/** 会话内保留组挂载（issue #893）：用全局 active pinia（全局壳层每测新建，报表页保留
 * 测试同款形态）——同一测试内先挂后挂共享同一会话，「卸载重挂」才表达「同一会话内
 * 离开再回来」；新 pinia 表达冷启动。 */
function mountSessionHarness() {
  const wrapper = mount(FilterHarness)
  return { wrapper: wrapper as VueWrapper, ...harness! }
}

function lastRequest(): TransactionListFilter {
  const requests = harness!.requests
  expect(requests.length, '应已发生至少一次重拉').toBeGreaterThan(0)
  return requests[requests.length - 1]
}

describe('useTransactionFilter 初始状态', () => {
  it('默认全量：五个过滤维度为 null，page=1，pageSize=20，版本号 0', () => {
    const { tf } = mountHarness()
    expect(tf.filters).toEqual({
      dateFrom: null,
      dateTo: null,
      involvingAccountId: null,
      merchantId: null,
      categoryId: null,
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
    tf.setFilter({ kinds: ['income'] })
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['income'])
    expect(tf.page.value).toBe(1)
    expect(tf.refreshVersion.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, kinds: ['income'] })
  })

  it('手动多选（spec #1025）：多类型集合生效，请求只携带集合参数（维度内取或）', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ kinds: ['buy', 'sell'] })
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['buy', 'sell'])
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, kinds: ['buy', 'sell'] })
  })

  it('选满全部可选类型：不归一为默认态（类型是可扩闭集），请求携带全量集合', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ kinds: [...TRANSACTION_KINDS] })
    await flushPromises()
    expect(tf.filters.kinds).toEqual([...TRANSACTION_KINDS])
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, kinds: [...TRANSACTION_KINDS] })
  })

  it('清空类型选择：回到不过滤（请求不再携带类型参数）', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ kinds: ['income'] })
    await flushPromises()
    tf.setFilter({ kinds: null })
    await flushPromises()
    expect(tf.filters.kinds).toBeNull()
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('同值意图不动作：条件实际变化才触发出口（不重拉、不归零）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    const kinds: TransactionKind[] = ['income']
    tf.setFilter({ kinds })
    await flushPromises()
    expect(requests).toHaveLength(1)
    // 再次声明同值意图（同一数组实例）→ 无变化即无出口
    tf.setFilter({ kinds })
    await flushPromises()
    expect(requests).toHaveLength(1)
    expect(tf.refreshVersion.value).toBe(1)
    expect(tf.page.value).toBe(1)
  })

  it('部分补丁合并：未提及的维度不受牵连', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01' })
    tf.setFilter({ kinds: ['expense'] })
    await flushPromises()
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kinds).toEqual(['expense'])
    expect(tf.filters.dateTo).toBeNull()
    expect(tf.filters.involvingAccountId).toBeNull()
  })

  it('多键补丁一次出口：一次翻页归零 + 一次重拉（中间态不产生多余请求）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 2
    tf.setFilter({ dateFrom: '2026-01-01', dateTo: '2026-01-31', kinds: ['income'] })
    await flushPromises()
    expect(tf.page.value).toBe(1)
    expect(requests).toHaveLength(1)
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      from: '2026-01-01',
      to: '2026-01-31',
      kinds: ['income'],
    })
  })

  it('多条件组合逐维声明：每次变化各走一次出口，最终请求参数完整', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1' })
    tf.setFilter({ dateFrom: '2026-01-01' })
    tf.setFilter({ dateTo: '2026-03-31' })
    tf.setFilter({ kinds: ['transfer'] })
    tf.setFilter({ merchantId: 'mch-1' })
    tf.setFilter({ categoryId: 'cat-1' })
    await flushPromises()
    expect(tf.filters).toEqual({
      dateFrom: '2026-01-01',
      dateTo: '2026-03-31',
      involvingAccountId: 'acc-1',
      merchantId: 'mch-1',
      categoryId: 'cat-1',
      kinds: ['transfer'],
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
      kinds: ['transfer'],
    })
  })
})

describe('useTransactionFilter resetFilters（清除筛选）', () => {
  it('有激活条件：全部维度回默认态 + 翻页归零 + 一次重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.setFilter({ involvingAccountId: 'acc-1', kinds: ['transfer'] })
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
    const { tf } = mountHarness()
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
    const { tf } = mountHarness()
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
    tf.setFilter({ involvingAccountId: 'acc-1', kinds: ['income'] })
    await flushPromises()
    const versionAfterFilter = tf.refreshVersion.value
    const requestsAfterFilter = requests.length
    tf.page.value = 2
    await flushPromises()
    expect(tf.page.value).toBe(2)
    // 组合不变量：翻页直写只动页码，过滤状态原样保留，调用方重拉即同时携带两者
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.filters.kinds).toEqual(['income'])
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

describe('useTransactionFilter 工厂形态（issue #893 起状态住交易页会话 store）', () => {
  it('同一会话内多次调用共享会话状态：可观察状态与版本号同源（会话内保留的载体）', async () => {
    const { tf: tf1 } = mountHarness()
    const tf2 = useTransactionFilter()
    await flushPromises()
    tf1.setFilter({ kinds: ['income'] })
    await flushPromises()
    expect(tf2.filters.kinds).toEqual(['income'])
    expect(tf2.refreshVersion.value).toBe(tf1.refreshVersion.value)
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
    const { tf } = mountHarness()
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
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kinds: ['income'] })
    await flushPromises()
    tf.syncUrlQuery({ account: 'missing-acc' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.kinds).toBeNull()
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('无效参数回退·merchant 维度同规则：字典中不存在的商户同样回退并复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kinds: ['income'] })
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'missing-mch' })
    await flushPromises()
    expect(tf.filters.merchantId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.kinds).toBeNull()
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
    const { tf } = mountHarness()
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
    const { tf } = mountHarness()
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
      kinds: null,
    })
    expect(tf.page.value).toBe(1)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })

  it('另一维度参数在场：被清除维度清空，日期/类型不越界复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1', merchant: 'mch-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kinds: ['income'] })
    await flushPromises()
    // 导航清除 account 参数（merchant 仍在场）
    tf.syncUrlQuery({ merchant: 'mch-1' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.merchantId).toBe('mch-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kinds).toEqual(['income'])
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      merchant_id: 'mch-1',
      from: '2026-01-01',
      kinds: ['income'],
    })
  })

  it('导航清除 merchant 参数：对应维度同步清空 + 日期/类型复位 + 翻页归零', async () => {
    const { tf } = mountHarness()
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
    tf.setFilter({ dateFrom: '2026-01-01', kinds: ['income'] })
    await flushPromises()
    tf.syncUrlQuery({ merchant: 'mch-1', account: 'missing-acc' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.merchantId).toBe('mch-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kinds).toEqual(['income'])
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
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: UNCATEGORIZED_ONLY })
    await flushPromises()
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, uncategorized_only: true })
  })

  it('软删分类 id 有效（历史交易口径）：校验应用不回退', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-2' }) // cat-2 为软删分类
    await flushPromises()
    expect(tf.filters.categoryId).toBe('cat-2')
    expect(lastRequest()).toEqual({ page: 1, page_size: 20, category_id: 'cat-2' })
  })

  it('未知分类 id：回退不过滤；另一维度（账户）有效在场时不误清其他维度、不越界复位', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kinds: ['income'] })
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1', category: 'missing-cat' })
    await flushPromises()
    expect(tf.filters.categoryId).toBeNull()
    expect(tf.filters.involvingAccountId).toBe('acc-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kinds).toEqual(['income'])
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
      from: '2026-01-01',
      kinds: ['income'],
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
    const { tf } = mountHarness()
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
    tf.setFilter({ dateFrom: '2026-01-01', kinds: ['income'] })
    await flushPromises()
    tf.syncUrlQuery({ category: UNCATEGORIZED_ONLY, account: 'missing-acc' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.categoryId).toBe(UNCATEGORIZED_ONLY)
    expect(tf.filters.dateFrom).toBe('2026-01-01')
    expect(tf.filters.kinds).toEqual(['income'])
  })

  it('导航换参：category 参数 id → 保留值，按新参数重新消费', async () => {
    const { tf } = mountHarness()
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

  it('报表跳转载荷形态：category + 当年首尾日期 + 类型集合组合直达，一次重拉（类型维度单维化，spec #1025）', async () => {
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

  it('无效日期回退触发复位契约：无其他有效参数时复位日期/类型（#96 决策 3，复位守卫按类型集合非空判定，spec #1025）', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    tf.setFilter({ dateFrom: '2026-01-01', kinds: ['income'] })
    await flushPromises()
    // 导航清除 account 且 dateFrom 变为非法：全部下钻参数均无效 → 复位日期/类型
    tf.syncUrlQuery({ dateFrom: 'not-a-date' })
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.dateFrom).toBeNull()
    expect(tf.filters.kinds).toBeNull()
    expect(lastRequest()).toEqual({ page: 1, page_size: 20 })
  })
})

// —— 类型维度（spec #1025 单维化，原 issue #581）：URL ?kinds= 逗号分隔闭集字面量；无参考数据
// 映射、不涉保留值，挂起补判/让位/复位守卫与既有维度同规。手动多选与下钻载荷共用同一维度，
// 载荷在场即覆盖手动多选（URL 永远赢）；与「仅无分类」解耦：仅无分类命中一切无分类交易、
// 不限定类型。

describe('useTransactionFilter URL 参数表·类型维度（手动多选 + 下钻共用，spec #1025）', () => {
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
    tf.setFilter({ dateFrom: '2026-01-01' })
    await flushPromises()
    tf.syncUrlQuery({ category: 'cat-1', kinds: 'transfer,bogus' })
    await flushPromises()
    expect(tf.filters.kinds).toBeNull()
    expect(tf.filters.categoryId).toBe('cat-1')
    expect(tf.filters.dateFrom).toBe('2026-01-01')
  })

  it('手动多选与下钻共用同一维度：URL 载荷在场即覆盖手动多选（URL 永远赢）', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.setFilter({ kinds: ['income'] })
    await flushPromises()
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['expense', 'refund'])
    expect(lastRequest()).toEqual({
      page: 1,
      page_size: 20,
      kinds: ['expense', 'refund'],
    })
  })

  it('URL 载荷未变化时不重放：手动多选不被同值重递静默覆盖', async () => {
    const { tf } = mountHarness()
    await flushPromises()
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    tf.setFilter({ kinds: ['income'] })
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['income'])
    // 同值重递（无关导航替换 query 对象）→ 该维度不动作，手动多选保留
    tf.syncUrlQuery({ kinds: 'expense,refund' })
    await flushPromises()
    expect(tf.filters.kinds).toEqual(['income'])
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
  wireInvokeSeam({ overrides: { [gatedCmd]: () => pending } })
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
    tf.setFilter({ kinds: ['income'] }) // 制造激活条件，使 resetFilters 实际动作
    tf.resetFilters()
    release()
    await flushPromises()
    expect(tf.filters.involvingAccountId).toBeNull()
    expect(tf.filters.kinds).toBeNull()
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

// —— 会话内保留（issue #893 / ADR-0094，spec #892）：筛选与分页住交易页会话 store ——
// 只测外部行为：「保留」的判据是同会话卸载重挂（同一 pinia 内重新挂载消费组件）后
// 状态恢复且以恢复状态重拉；「冷启动」的判据是新 pinia 实例回默认；「零写盘」的判据
// 是全程无 localStorage 写入（报表页保留测试先例的三锚点，spec #892 测试决策）。
describe('useTransactionFilter 会话内保留（issue #893）：同会话卸载重挂恢复，新会话冷启动', () => {
  it('筛选全维 + 翻页 + 换页大小后卸载重挂（同一会话）：状态恢复，恢复首刷以恢复状态重拉不翻页', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({
      involvingAccountId: 'acc-1',
      kinds: ['income'],
      dateFrom: '2026-01-01',
      dateTo: '2026-03-31',
    })
    first.tf.page.value = 3
    first.tf.pageSize.value = 50
    await flushPromises()
    first.wrapper.unmount()

    // 同一会话重挂（侧栏往返/下钻往返/回退）：离开时的选择原样恢复
    const second = mountSessionHarness()
    await flushPromises()
    expect(second.tf.filters).toEqual({
      dateFrom: '2026-01-01',
      dateTo: '2026-03-31',
      involvingAccountId: 'acc-1',
      merchantId: null,
      categoryId: null,
      kinds: ['income'],
    })
    expect(second.tf.page.value).toBe(3)
    expect(second.tf.pageSize.value).toBe(50)
    // 恢复访次的重拉（以恢复状态现拉，不翻页）由消费方首拉承担：视图层锚定
    // （TransactionsView session-retention 测试）；模块边界只产出状态与版本信号
    second.wrapper.unmount()
  })

  it('新 pinia 表达冷启动：回默认无筛选态、第 1 页、默认页大小，无恢复首刷', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({ merchantId: 'mch-1', kinds: ['income'] })
    first.tf.page.value = 2
    await flushPromises()
    first.wrapper.unmount()

    // 新 pinia = 新会话（应用重启）：回默认
    setActivePinia(createPinia())
    const second = mountSessionHarness()
    await flushPromises()
    expect(second.tf.filters).toEqual({
      dateFrom: null,
      dateTo: null,
      involvingAccountId: null,
      merchantId: null,
      categoryId: null,
      kinds: null,
    })
    expect(second.tf.page.value).toBe(1)
    expect(second.tf.pageSize.value).toBe(20)
    expect(second.requests).toHaveLength(0)
    second.wrapper.unmount()
  })

  it('零持久化：选择筛选与卸载重挂全程 localStorage 零写入', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    const keysBefore = Object.keys(localStorage)
    first.tf.setFilter({ involvingAccountId: 'acc-1', kinds: ['transfer'] })
    first.tf.page.value = 2
    await flushPromises()
    first.wrapper.unmount()
    const second = mountSessionHarness()
    await flushPromises()
    expect(second.tf.filters.involvingAccountId).toBe('acc-1')
    expect(Object.keys(localStorage)).toEqual(keysBefore)
    second.wrapper.unmount()
  })

  it('URL 下钻参数在场永远赢：覆盖保留态对应维度（含手动多选的类型维度），无参数维度恢复保留态', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({ merchantId: 'mch-1', kinds: ['income'] })
    first.tf.page.value = 2
    await flushPromises()
    first.wrapper.unmount()

    const second = mountSessionHarness()
    second.tf.syncUrlQuery({ account: 'acc-1', kinds: 'expense,refund' })
    await flushPromises()
    // URL 参数维度按参数装配（显式跳转意图）；无参数维度保留离开时的选择
    expect(second.tf.filters.involvingAccountId).toBe('acc-1')
    expect(second.tf.filters.merchantId).toBe('mch-1')
    // 类型维度单维化：下钻载荷在场即覆盖手动多选（URL 永远赢，spec #1025）
    expect(second.tf.filters.kinds).toEqual(['expense', 'refund'])
    expect(second.tf.page.value).toBe(1)
    // URL 应用走统一出口：一次重拉、参数完整（恢复态的重拉由消费方首拉承担）
    expect(second.requests).toHaveLength(1)
    expect(second.requests[0]).toEqual({
      page: 1,
      page_size: 20,
      involving_account_id: 'acc-1',
      merchant_id: 'mch-1',
      kinds: ['expense', 'refund'],
    })
    second.wrapper.unmount()
  })

  it('URL 不带类型参数：手动多选跨导航保留（侧栏往返不丢选择）', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({ kinds: ['buy', 'sell'] })
    first.tf.page.value = 2
    await flushPromises()
    first.wrapper.unmount()

    const second = mountSessionHarness()
    second.tf.syncUrlQuery({})
    await flushPromises()
    expect(second.tf.filters.kinds).toEqual(['buy', 'sell'])
    expect(second.tf.page.value).toBe(2)
    second.wrapper.unmount()
  })

  it('URL 参数命中保留态同维度：跨访问的旧手动状态不抗参数（让位守卫仅限单次进入内）', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({ involvingAccountId: 'acc-2' }) // 上一次访问留下的手动状态
    await flushPromises()
    first.wrapper.unmount()

    const second = mountSessionHarness()
    second.tf.syncUrlQuery({ account: 'acc-1' })
    await flushPromises()
    expect(second.tf.filters.involvingAccountId).toBe('acc-1')
    second.wrapper.unmount()
  })

  it('无参数恢复保留态：不带参重挂保留筛选与页码（侧栏往返与下钻往返互不干扰）', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({ involvingAccountId: 'acc-1' })
    first.tf.page.value = 2
    await flushPromises()
    first.wrapper.unmount()

    const second = mountSessionHarness()
    second.tf.syncUrlQuery({}) // 视图 immediate 转发当前无参 query
    await flushPromises()
    expect(second.tf.filters.involvingAccountId).toBe('acc-1')
    expect(second.tf.page.value).toBe(2)
    // 无参数：URL 出口不动作，恢复态原样（重拉由消费方首拉承担）
    expect(second.requests).toHaveLength(0)
    second.wrapper.unmount()
  })

  it('纯翻页偏离（无筛选）也是保留态：重挂恢复页码；复位出口对纯翻页同样归零', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.page.value = 3
    await flushPromises()
    first.wrapper.unmount()

    const second = mountSessionHarness()
    await flushPromises()
    expect(second.tf.page.value).toBe(3)
    // 复位出口的激活判定含纯翻页偏离：ESC 复位归零 + 重拉
    second.tf.resetFilters()
    await flushPromises()
    expect(second.tf.page.value).toBe(1)
    expect(second.requests).toHaveLength(1)
    expect(second.requests[0]).toEqual({ page: 1, page_size: 20 })
    second.wrapper.unmount()
  })

  it('复位出口清除保留态本身：复位后卸载重挂 = 默认态、无恢复首刷（story #10）', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({ involvingAccountId: 'acc-1', kinds: ['income'] })
    first.tf.page.value = 2
    await flushPromises()
    first.tf.resetFilters() // ESC 复位走模块既有复位出口
    await flushPromises()
    expect(first.tf.page.value).toBe(1)
    first.wrapper.unmount()

    const second = mountSessionHarness()
    await flushPromises()
    expect(second.tf.filters).toEqual({
      dateFrom: null,
      dateTo: null,
      involvingAccountId: null,
      merchantId: null,
      categoryId: null,
      kinds: null,
    })
    expect(second.tf.page.value).toBe(1)
    expect(second.requests).toHaveLength(0)
    second.wrapper.unmount()
  })

  it('恢复访次内 refresh 语义不变：外部数据变化仍翻回第一页，筛选保留', async () => {
    const first = mountSessionHarness()
    await flushPromises()
    first.tf.setFilter({ involvingAccountId: 'acc-1' })
    first.tf.page.value = 3
    await flushPromises()
    first.wrapper.unmount()

    const second = mountSessionHarness()
    await flushPromises()
    expect(second.tf.page.value).toBe(3)
    second.tf.refresh() // 记一笔提交等外部数据变化回填
    await flushPromises()
    expect(second.tf.page.value).toBe(1)
    expect(second.tf.filters.involvingAccountId).toBe('acc-1')
    expect(second.requests).toEqual([{ page: 1, page_size: 20, involving_account_id: 'acc-1' }])
    second.wrapper.unmount()
  })
})
