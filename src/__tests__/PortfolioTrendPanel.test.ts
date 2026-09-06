import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import PortfolioTrendPanel from '@/components/investments/PortfolioTrendPanel.vue'
import { makeInstrument } from './factories'
import {
  firePricesChanged,
  resetPricesChangedHandler,
} from './prices-changed-mock'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { PortfolioValueTrend } from '@/types'

vi.mock('vue-chartjs', async () => {
  const { LineChartStub } = await import('./line-chart-stub')
  return { Line: LineChartStub }
})

const mockListen = vi.mocked(listen)

// 价格失效信号订阅基座 mock（issue #238 / ADR-0031 决策 3）：捕获订阅回调，
// 测试中手动触发模拟后端 emit；捕获/触发辅助收在 prices-changed-mock 共享。
vi.mock('@/composables/usePricesChanged', async () => {
  const { capturePricesChangedHandler } = await import('./prices-changed-mock')
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  }
})

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const portfolioTrend: PortfolioValueTrend = {
  currency_code: 'CNY',
  points: [
    { date: '2026-06-05', market_value_cents: 100000 },
    { date: '2026-06-12', market_value_cents: 110000 },
  ],
}

const stockInstrument = makeInstrument({
  id: 'inst-1',
  symbol: '600000',
  name: '浦发银行',
  type: 'stock',
  market: 'sh',
})

const fundInstrument = makeInstrument({
  id: 'inst-fund',
  symbol: '000198',
  name: '天弘余额宝',
  type: 'fund',
  market: 'unknown',
})

function baseInvoke(extra?: Record<string, unknown>) {
  stubReferenceInvoke({
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
    list_holdings: [],
    list_instruments: { items: [stockInstrument, fundInstrument], total: 2 },
    portfolio_value_trend: () => Promise.resolve(portfolioTrend),
    ...extra,
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  mockListen.mockResolvedValue(() => {})
  resetPricesChangedHandler()
  baseInvoke()
  const store = useReferenceStore()
  await store.refresh()
})

function chartPayload(wrapper: ReturnType<typeof mount>): { labels: string[]; datasets: { data: number[] }[] } {
  const el = wrapper.get('[data-testid="line-chart"]')
  return JSON.parse(el.text())
}

describe('PortfolioTrendPanel 走势面板', () => {
  it('默认组合模式：拉取组合市值并渲染图表序列与本位币标注', async () => {
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    const call = mockInvoke.mock.calls.find(([c]) => c === 'portfolio_value_trend')
    expect(call).toBeTruthy()
    const payload = chartPayload(wrapper)
    expect(payload.labels).toEqual(['2026-06-05', '2026-06-12'])
    expect(payload.datasets[0].data).toEqual([100000, 110000])
    // 本位币口径标注
    expect(wrapper.get('[data-testid="trend-currency"]').text()).toContain('CNY')
    expect(wrapper.get('[data-testid="trend-currency"]').text()).toContain('本位币')
  })

  it('渲染模式切换与预设区间（1 月 / 3 月 / 1 年 / 全部）', async () => {
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    const text = wrapper.text()
    for (const label of ['组合市值', '单标的', '1 月', '3 月', '1 年', '全部']) {
      expect(text).toContain(label)
    }
  })

  it('预设区间切换后重新查询并携带新的起始日期', async () => {
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    const before = mockInvoke.mock.calls.filter(([c]) => c === 'portfolio_value_trend').length
    // 点击「1 月」预设
    const radio = wrapper.findAll('.n-radio').find((r) => r.text() === '1 月')
    expect(radio).toBeTruthy()
    await radio!.find('input').setValue(true)
    await flushPromises()
    const after = mockInvoke.mock.calls.filter(([c]) => c === 'portfolio_value_trend').length
    expect(after).toBeGreaterThan(before)
    const last = mockInvoke.mock.calls.filter(([c]) => c === 'portfolio_value_trend').at(-1)!
    expect((last[1] as { filter: { start_date: string } }).filter.start_date).toBeTruthy()
  })

  it('组合走势无数据 → 引导文案提示去「同步持仓价格」', async () => {
    baseInvoke({ portfolio_value_trend: { currency_code: 'CNY', points: [] } })
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    expect(wrapper.text()).toContain('暂无历史价格数据')
    expect(wrapper.text()).toContain('同步持仓价格')
    expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false)
  })

  it('标的列表带入单标的：以标的 id 查询并标注计价币种', async () => {
    baseInvoke({
      instrument_price_trend: {
        instrument_id: 'inst-1',
        points: [
          { date: '2026-06-05', price_cents: 1500, currency_code: 'CNY' },
        ],
      },
    })
    const wrapper = mount(PortfolioTrendPanel, {
      props: { entryInstrument: stockInstrument },
    })
    await flushPromises()
    const call = mockInvoke.mock.calls.filter(([c]) => c === 'instrument_price_trend').at(-1)!
    expect((call[1] as { instrumentId: string }).instrumentId).toBe('inst-1')
    const payload = chartPayload(wrapper)
    expect(payload.datasets[0].data).toEqual([1500])
    expect(wrapper.get('[data-testid="trend-currency"]').text()).toContain('CNY')
  })

  it('非股票标的 → 「暂无行情来源」边界说明，不发起走势查询', async () => {
    const wrapper = mount(PortfolioTrendPanel, {
      props: { entryInstrument: fundInstrument },
    })
    await flushPromises()
    expect(wrapper.text()).toContain('暂无行情来源')
    expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'instrument_price_trend')).toBe(false)
  })

  it('价格失效信号触发后重拉走势：键（模式+区间）未变也强制重取（issue #238）', async () => {
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    const before = mockInvoke.mock.calls.filter(([c]) => c === 'portfolio_value_trend').length
    firePricesChanged()
    await flushPromises()
    // 同步写价后键未变，但同键去重短路必须让位于信号重拉，否则走势留陈旧点
    const calls = mockInvoke.mock.calls.filter(([c]) => c === 'portfolio_value_trend')
    expect(calls.length).toBe(before + 1)
    // 图表序列随重拉结果刷新
    expect(chartPayload(wrapper).datasets[0].data).toEqual([100000, 110000])
  })
})
