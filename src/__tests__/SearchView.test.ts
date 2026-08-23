import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'
import SearchView from '@/views/SearchView.vue'
import type { Account, Category, Currency, Transaction } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-cash',
    name: '现金',
    type: 'cash',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

const mockCategories: Category[] = [
  {
    id: 'cat-food',
    name: '餐饮',
    kind: 'expense',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

function makeTransaction(id: string, note: string, date: string): Transaction {
  return {
    id,
    kind: 'expense',
    amount_cents: 2500,
    currency_code: 'CNY',
    amount_native_cents: 2500,
    account_id: 'acc-cash',
    to_account_id: null,
    category_id: 'cat-food',
    refund_of_transaction_id: null,
    note,
    date,
    created_at: `${date}T00:00:00Z`,
    updated_at: `${date}T00:00:00Z`,
    version: 1,
    device_id: 'test',
    is_deleted: false,
  }
}

// 25 条：前 23 条备注「午餐」、后 2 条备注「报销」（跨 2 页，pageSize=20）
const mockTransactions: Transaction[] = Array.from({ length: 25 }, (_, i) =>
  makeTransaction(
    `tx-${i + 1}`,
    i < 23 ? '午餐' : '报销',
    `2026-02-${String(i + 1).padStart(2, '0')}`,
  ),
)

function searchCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === 'search_transactions')
}

function lastSearchArgs() {
  const calls = searchCalls()
  expect(calls.length).toBeGreaterThan(0)
  return calls[calls.length - 1][1] as { query: string; page: number; pageSize: number }
}

/** 输入关键字并等待防抖与异步搜索完成（fake timers 下微任务由 advance 驱动）。 */
async function typeAndSearch(wrapper: VueWrapper, text: string, delay = 300) {
  await wrapper.find('input').setValue(text)
  await nextTick()
  await vi.advanceTimersByTimeAsync(delay)
  await nextTick()
  await nextTick()
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve(mockCategories)
    if (cmd === 'search_transactions') {
      const { query, page = 1, pageSize = 20 } = (args ?? {}) as {
        query?: string
        page?: number
        pageSize?: number
      }
      if (!query) return Promise.resolve({ items: [], total: 0 })
      const all = mockTransactions.filter((t) => (t.note ?? '').includes(query))
      const start = (page - 1) * pageSize
      return Promise.resolve({ items: all.slice(start, start + pageSize), total: all.length })
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useAppStore()
  await store.loadAll()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('SearchView.vue', () => {
  it('空输入显示占位提示且不触发搜索', async () => {
    const wrapper = mount(SearchView)
    await flushPromises()
    expect(wrapper.text()).toContain('输入关键字开始搜索')
    expect(searchCalls().length).toBe(0)
  })

  it('输入后防抖 300ms 才触发一次搜索', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '午餐', 299)
    expect(searchCalls().length).toBe(0)
    await vi.advanceTimersByTimeAsync(1)
    await nextTick()
    await nextTick()
    expect(searchCalls().length).toBe(1)
  })

  it('连续输入只触发一次搜索（防抖合并）', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await wrapper.find('input').setValue('午')
    await nextTick()
    await vi.advanceTimersByTimeAsync(100)
    await wrapper.find('input').setValue('午餐')
    await nextTick()
    await vi.advanceTimersByTimeAsync(300)
    await nextTick()
    await nextTick()
    expect(searchCalls().length).toBe(1)
    expect(lastSearchArgs().query).toBe('午餐')
  })

  it('搜索结果渲染表格并显示「命中 N 条」', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '报销')
    expect(wrapper.text()).toContain('命中 2 条')
    expect(wrapper.text()).toContain('报销')
    expect(wrapper.text()).toContain('¥25.00')
    expect(wrapper.text()).toContain('支出')
    expect(wrapper.text()).toContain('餐饮')
    expect(wrapper.text()).toContain('现金')
  })

  it('invoke(search_transactions) 参数正确（query/page/pageSize）', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '午餐')
    expect(lastSearchArgs()).toMatchObject({ query: '午餐', page: 1, pageSize: 20 })
  })

  it('分页：点击第 2 页携带 page=2 重新搜索', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '午餐')
    expect(wrapper.text()).toContain('命中 23 条')

    const pageTwo = wrapper
      .findAll('.n-pagination-item')
      .find((el) => el.text() === '2')
    expect(pageTwo).toBeTruthy()
    await pageTwo!.trigger('click')
    await nextTick()
    await nextTick()

    expect(lastSearchArgs()).toMatchObject({ query: '午餐', page: 2, pageSize: 20 })
  })

  it('无匹配结果显示「命中 0 条」与空态提示', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '不存在的关键字')
    expect(wrapper.text()).toContain('命中 0 条')
    expect(wrapper.text()).toContain('无匹配结果')
  })

  it('结果只读：不渲染删除操作', async () => {
    vi.useFakeTimers()
    const wrapper = mount(SearchView)
    await nextTick()
    await typeAndSearch(wrapper, '报销')
    expect(wrapper.text()).not.toContain('删除')
  })
})
