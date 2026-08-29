import { beforeEach, describe, expect, it, vi } from 'vitest'
import { defineComponent, watch } from 'vue'
import { flushPromises, mount } from '@vue/test-utils'
import { createTestingPinia } from '@pinia/testing'
import { invoke } from '@tauri-apps/api/core'
import { useTransactionFilter } from '@/composables/useTransactionFilter'
import type { UseTransactionFilterReturn } from '@/composables/useTransactionFilter'
import type { TransactionListFilter } from '@/types'

const mockInvoke = vi.mocked(invoke)

beforeEach(() => {
  // Reference Data store 以 createTestingPinia 标准 mock（ADR-0030 决策 7）：
  // 本 ticket 模块尚未消费 store（#234 内化 URL 链路时接入），此处仅供 pinia 环境
  // 并兜底 store self-init 的 invoke（空参考表即可）。
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
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
      if (tf.filters.kind) f.kind = tf.filters.kind
      requests.push(f)
    })
    harness = { tf, requests }
    return () => null
  },
})

function mountHarness() {
  mount(FilterHarness, { global: { plugins: [createTestingPinia()] } })
  return harness!
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
      kind: null,
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
    await flushPromises()
    expect(tf.filters).toEqual({
      dateFrom: '2026-01-01',
      dateTo: '2026-03-31',
      involvingAccountId: 'acc-1',
      merchantId: 'mch-1',
      kind: 'transfer',
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
      kind: null,
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
  it('页大小切换经 refresh 出口：归零 + 以新页大小重拉', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.pageSize.value = 50
    tf.refresh()
    await flushPromises()
    expect(lastRequest()).toEqual({ page: 1, page_size: 50 })
  })

  it('翻页导航由调用方直写页码：版本号不 bump（视图自行以新页码重拉）', async () => {
    const { tf, requests } = mountHarness()
    await flushPromises()
    tf.page.value = 2
    await flushPromises()
    expect(tf.page.value).toBe(2)
    expect(tf.refreshVersion.value).toBe(0)
    expect(requests).toHaveLength(0)
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
