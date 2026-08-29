import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import InvestmentsView from '@/views/InvestmentsView.vue'
import type { Currency, Instrument } from '@/types'

// 走势图用共享桩组件替代：组件层测试只验证数据联动与文案渲染，不验证 canvas 绘制
vi.mock('vue-chartjs', async () => {
  const { LineChartStub } = await import('./line-chart-stub')
  return { Line: LineChartStub }
})

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
    if (cmd === 'list_merchants') return Promise.resolve([])
    // 持仓概览（issue #110）：盈亏 tab 顶部会拉取当前持仓
    if (cmd === 'list_holdings') return Promise.resolve([])
    // 走势（issue #139）：标的列表「走势」入口切入走势 tab 时由面板拉取
    if (cmd === 'portfolio_value_trend')
      return Promise.resolve({ currency_code: 'CNY', points: [] })
    if (cmd === 'instrument_price_trend')
      return Promise.resolve({
        instrument_id: 'inst-1',
        points: [{ date: '2026-06-05', price_cents: 1500, currency_code: 'CNY' }],
      })
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
  const store = useReferenceStore()
  await store.refresh()
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

  it('走势 tab 存在', () => {
    const wrapper = mount(InvestmentsView)
    const labels = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(labels).toContain('走势')
  })

  it('标的列表「走势」入口：切到走势 tab 并以单标的模式查询该标的', async () => {
    const wrapper = mount(InvestmentsView)
    await nextTick()
    // 进入标的 tab
    await wrapper.findAll('.n-tabs-tab')[1].trigger('click')
    await nextTick()
    await nextTick()
    // 点第一行（600000 浦发银行）的「走势」按钮
    const btn = wrapper.find('[data-testid="view-trend-600000"]')
    expect(btn.exists()).toBe(true)
    await btn.trigger('click')
    await nextTick()
    await nextTick()
    // tab 已切到走势，面板以单标的模式查询该标的
    const call = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'instrument_price_trend').at(-1)
    expect(call).toBeTruthy()
    expect((call![1] as { instrumentId: string }).instrumentId).toBe('inst-1')
    expect(wrapper.get('[data-testid="line-chart"]').text()).toContain('1500')
  })

  it('离开走势 tab 清空入口标的：直入「走势」tab 回到默认组合曲线', async () => {
    const wrapper = mount(InvestmentsView)
    await nextTick()
    // 经标的列表入口进入单标的走势
    await wrapper.findAll('.n-tabs-tab')[1].trigger('click')
    await nextTick()
    await nextTick()
    await wrapper.find('[data-testid="view-trend-600000"]').trigger('click')
    await nextTick()
    await nextTick()
    const instCallsAfterEntry = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === 'instrument_price_trend',
    ).length
    expect(instCallsAfterEntry).toBe(1)
    // 切到盈亏再直入走势：入口残留已清空，回到组合模式（无新的单标的查询）
    await wrapper.findAll('.n-tabs-tab')[0].trigger('click')
    await nextTick()
    await nextTick()
    await wrapper.findAll('.n-tabs-tab')[2].trigger('click')
    await flushPromises()
    const instCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'instrument_price_trend')
    expect(instCalls.length).toBe(1)
    // 组合走势空数据 → 引导文案（而非上一标的的单标的曲线）
    expect(wrapper.text()).toContain('暂无历史价格数据')
  })
})
