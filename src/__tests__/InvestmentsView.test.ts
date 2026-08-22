import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useAppStore } from '@/stores/app'
import InvestmentsView from '@/views/InvestmentsView.vue'
import type { Currency, Instrument } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockInstruments: Instrument[] = [
  {
    id: 'inst-1',
    symbol: '600000',
    type: 'stock',
    name: '浦发银行',
    currency_code: 'CNY',
    market: 'sh',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'inst-2',
    symbol: '000001',
    type: 'stock',
    name: '平安银行',
    currency_code: 'CNY',
    market: 'sz',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
  {
    id: 'inst-3',
    symbol: '00700',
    type: 'stock',
    name: '腾讯控股',
    currency_code: 'HKD',
    market: 'hk',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_instruments')
      return Promise.resolve({ items: mockInstruments, total: mockInstruments.length })
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'realized_pnl_summary')
      return Promise.resolve({
        total_realized_pnl_cents: 0,
        by_year: [],
        by_account: [],
        by_instrument: [],
        details: [],
      })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  localStorage.clear()
  const store = useAppStore()
  await store.loadAll()
})

describe('InvestmentsView 标的 tab', () => {
  it('盈亏 tab 存在', async () => {
    const wrapper = mount(InvestmentsView)
    await nextTick()
    const tabs = wrapper.findAll('.n-tabs-tab')
    const labels = tabs.map((t) => t.text())
    expect(labels).toContain('盈亏')
  })

  it('标的 tab 存在', () => {
    const wrapper = mount(InvestmentsView)
    const tabs = wrapper.findAll('.n-tabs-tab')
    const labels = tabs.map((t) => t.text())
    expect(labels).toContain('标的')
  })

  it('标的 tab 显示标的搜索框', async () => {
    const wrapper = mount(InvestmentsView)
    await nextTick()
    await nextTick()
    const instTab = wrapper.findAll('.n-tabs-tab')[1]
    await instTab.trigger('click')
    await nextTick()
    await nextTick()
    expect(wrapper.html()).toContain('搜索代码或名称')
  })

  it('标的 tab 支持搜索', async () => {
    const wrapper = mount(InvestmentsView)
    await nextTick()
    const instTab = wrapper.findAll('.n-tabs-tab')[1]
    await instTab.trigger('click')
    await nextTick()
    await nextTick()
    expect(wrapper.html()).toContain('搜索')
  })

  it('标的 tab 包含市场筛选', async () => {
    const wrapper = mount(InvestmentsView)
    await nextTick()
    const instTab = wrapper.findAll('.n-tabs-tab')[1]
    await instTab.trigger('click')
    await nextTick()
    await nextTick()
    expect(wrapper.html()).toContain('全部市场')
  })

  it('标的 tab 分页请求携带 page/page_size', async () => {
    const wrapper = mount(InvestmentsView)
    await nextTick()
    const instTab = wrapper.findAll('.n-tabs-tab')[1]
    await instTab.trigger('click')
    await nextTick()
    await nextTick()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    expect(calls.length).toBeGreaterThan(0)
    const [, args] = calls[calls.length - 1]
    expect(args.filter).toMatchObject({ page: 1, page_size: 50 })
  })
})
