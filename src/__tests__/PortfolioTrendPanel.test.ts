import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { useReferenceStore } from '@/stores/reference'
import { useInvestmentsSessionStore } from '@/investment/investments-session'
import PortfolioTrendPanel from '@/investment/PortfolioTrendPanel.vue'
import { componentVm } from '@ledger/test-support/component-vm'
import { hoverTipText } from '@ledger/test-support/tooltip'
import { makeInstrument } from './factories'
import {
  firePricesChanged,
  resetPricesChangedHandler,
} from './prices-changed-mock'
import type { Instrument, PortfolioValueTrend } from '@ledger/types'

vi.mock('vue-chartjs', async () => {
  const { LineChartStub } = await import('./line-chart-stub')
  return { Line: LineChartStub }
})

// 价格失效信号订阅基座 mock（issue #238 / ADR-0031 决策 3）：捕获订阅回调，
// 测试中手动触发模拟后端 emit；捕获/触发辅助收在 prices-changed-mock 共享。
vi.mock('@/investment/usePricesChanged', async () => {
  const { capturePricesChangedHandler } = await import('./prices-changed-mock')
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  }
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
  price_channel: 'quote',
})

const fundInstrument = makeInstrument({
  id: 'inst-fund',
  symbol: '000198',
  name: '天弘余额宝',
  type: 'fund',
  market: 'unknown',
  price_channel: 'fund_nav',
})

/** 录过价的自建标的（且慢组合形态）：手动报价通道（issue #291） */
const manualInstrument = makeInstrument({
  id: 'inst-manual',
  symbol: '稳稳地幸福',
  name: '且慢组合',
  type: 'other',
  market: 'unknown',
  price_channel: 'manual',
})

/** 市场未知的股票类：无任何价格来源（issue #1060） */
const noSourceInstrument = makeInstrument({
  id: 'inst-none',
  symbol: 'ghost1',
  name: '幽灵股票',
  type: 'stock',
  market: 'unknown',
  price_channel: 'none',
})

/** 面板挂载即拉的领域命令契约快照：持仓空集 + 两标的字典。 */
const PANEL_DEFAULTS = {
  list_holdings: [],
  list_instruments: { items: [stockInstrument, fundInstrument], total: 2 },
}

/** 组合走势默认应答（函数型，保持既有形态）。 */
const portfolioTrendResponse = () => Promise.resolve(portfolioTrend)

beforeEach(async () => {
  resetPricesChangedHandler()
  wireInvokeSeam({ defaults: PANEL_DEFAULTS, overrides: { portfolio_value_trend: portfolioTrendResponse } })
  const store = useReferenceStore()
  await store.refresh()
})

/** 走势入口（标的列表「走势」按钮 / focus 落点同款）：写会话 store 后挂载面板——
 * 入口标的是会话保留态，面板不再持入口 props（issue #1192）。 */
function mountWithEntry(instrument: Instrument) {
  useInvestmentsSessionStore().showTrendInstrument(instrument)
  return mount(PortfolioTrendPanel)
}

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

  it('组合走势无数据且无补全状态 → 中性空态，不出现指向同步按钮的回填文案', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: { currency_code: 'CNY', points: [] },
      },
    })
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    expect(wrapper.text()).toContain('暂无历史价格数据')
    // 回填文案退场（issue #1377）：同步只刷现价，历史归后台补全——按了也不会
    // 立即有曲线，指向按钮即死路
    expect(wrapper.text()).not.toContain('同步标的信息')
    expect(wrapper.text()).toContain('更长的区间')
    expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false)
  })

  it('组合走势无数据且库内有待补全标的 → 空态三态：补全中（带计数）', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: {
          currency_code: 'CNY',
          points: [],
          backfill: { state: 'running', done: 13, total: 228 },
        },
      },
    })
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    const extra = wrapper.get('[data-testid="trend-empty-backfill"]')
    expect(extra.text()).toContain('补全中')
    expect(extra.text()).toContain('13/228')
    // 三态均不指向同步按钮（存在性断言：删掉三态分支即变红）
    expect(wrapper.text()).not.toContain('同步标的信息')
  })

  it('标的列表带入单标的：以标的 id 查询并标注计价币种', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: portfolioTrendResponse,
        instrument_price_trend: {
          instrument_id: 'inst-1',
          points: [
            { date: '2026-06-05', price_cents: 1500, currency_code: 'CNY' },
          ],
        },
      },
    })
    const wrapper = mountWithEntry(stockInstrument)
    await flushPromises()
    const call = mockInvoke.mock.calls.filter(([c]) => c === 'instrument_price_trend').at(-1)!
    expect((call[1] as { instrumentId: string }).instrumentId).toBe('inst-1')
    const payload = chartPayload(wrapper)
    expect(payload.datasets[0].data).toEqual([1500])
    expect(wrapper.get('[data-testid="trend-currency"]').text()).toContain('CNY')
  })

  it('无价格来源标的（通道 = none）→ 「没有价格来源」边界说明，不发起走势查询、不出图', async () => {
    const wrapper = mountWithEntry(noSourceInstrument)
    await flushPromises()
    expect(wrapper.find('[data-testid="trend-no-source"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('没有价格来源')
    // 边界说明不再误述「仅股票 / ETF 支持」：说明的是没有价格来源
    expect(wrapper.text()).not.toContain('仅股票 / ETF 支持')
    expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false)
    expect(mockInvoke.mock.calls.some(([c]) => c === 'instrument_price_trend')).toBe(false)
  })

  it('选中场外基金（净值通道）→ 净值曲线出图（#303 验收在界面上成立）', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: portfolioTrendResponse,
        instrument_price_trend: {
          instrument_id: 'inst-fund',
          points: [
            { date: '2026-06-05', price_cents: 12850, currency_code: 'CNY' },
            { date: '2026-06-12', price_cents: 12960, currency_code: 'CNY' },
          ],
        },
      },
    })
    const wrapper = mountWithEntry(fundInstrument)
    await flushPromises()
    // 发起净值走势查询并出图（此前被前端场内白名单拦截）
    const call = mockInvoke.mock.calls.filter(([c]) => c === 'instrument_price_trend').at(-1)!
    expect((call[1] as { instrumentId: string }).instrumentId).toBe('inst-fund')
    const payload = chartPayload(wrapper)
    expect(payload.datasets[0].data).toEqual([12850, 12960])
    expect(wrapper.find('[data-testid="trend-no-source"]').exists()).toBe(false)
  })

  it('选中录过价的自建标的（手动报价通道）→ 价格曲线出图（#291 验收在界面上成立）', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: portfolioTrendResponse,
        instrument_price_trend: {
          instrument_id: 'inst-manual',
          points: [
            { date: '2026-06-05', price_cents: 13180, currency_code: 'CNY' },
            { date: '2026-06-12', price_cents: 13200, currency_code: 'CNY' },
          ],
        },
      },
    })
    const wrapper = mountWithEntry(manualInstrument)
    await flushPromises()
    const call = mockInvoke.mock.calls.filter(([c]) => c === 'instrument_price_trend').at(-1)!
    expect((call[1] as { instrumentId: string }).instrumentId).toBe('inst-manual')
    const payload = chartPayload(wrapper)
    expect(payload.datasets[0].data).toEqual([13180, 13200])
    expect(wrapper.find('[data-testid="trend-no-source"]').exists()).toBe(false)
  })

  it('有通道无数据（单标的）：空态三态——补全中 / 待重试 / 无数据，均不指向同步按钮', async () => {
    // 三态由后端读投影只增字段携带（issue #1377）：前端按 state 分派文案。
    const cases: Array<{
      backfill: { state: 'running' | 'retry_pending' | 'no_data'; done?: number; total?: number }
      expectText: string
      notText?: string
    }> = [
      {
        backfill: { state: 'running', done: 1, total: 3 },
        expectText: '补全中（1/3）',
      },
      {
        backfill: { state: 'running' },
        expectText: '补全中',
        notText: '补全中（',
      },
      {
        backfill: { state: 'retry_pending' },
        expectText: '自动重试',
      },
      {
        backfill: { state: 'no_data' },
        expectText: '没有可采集的历史数据',
      },
    ]
    for (const item of cases) {
      wireInvokeSeam({
        defaults: PANEL_DEFAULTS,
        overrides: {
          portfolio_value_trend: portfolioTrendResponse,
          instrument_price_trend: { instrument_id: 'inst-fund', points: [], backfill: item.backfill },
        },
      })
      const wrapper = mountWithEntry(fundInstrument)
      await flushPromises()
      expect(wrapper.find('[data-testid="trend-empty-backfill"]').exists()).toBe(true)
      const extra = wrapper.get('[data-testid="trend-empty-backfill"]')
      expect(extra.text()).toContain(item.expectText)
      if (item.notText) expect(extra.text()).not.toContain(item.notText)
      // 空态不再出现指向同步按钮的回填文案（按了没反应的死路）
      expect(wrapper.text()).not.toContain('同步标的信息')
      expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false)
      wrapper.unmount()
    }
  })

  it('有历史序列的标的区间裁剪到空：不携带补全状态，给中性文案', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: portfolioTrendResponse,
        instrument_price_trend: { instrument_id: 'inst-fund', points: [] },
      },
    })
    const wrapper = mountWithEntry(fundInstrument)
    await flushPromises()
    expect(wrapper.find('[data-testid="trend-empty-neutral"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('更长的区间')
    expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false)
  })

  it('有通道无数据：手动报价通道引导去「录价」，与补全三态是两种不同文案', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: portfolioTrendResponse,
        instrument_price_trend: { instrument_id: 'inst-manual', points: [] },
      },
    })
    const wrapper = mountWithEntry(manualInstrument)
    await flushPromises()
    expect(wrapper.find('[data-testid="trend-empty"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('暂无历史价格数据')
    expect(wrapper.text()).toContain('录价')
    expect(wrapper.text()).not.toContain('同步标的信息')
    // 手动通道不归后台补全：后端不携带补全状态，前端也不显示三态文案
    expect(wrapper.find('[data-testid="trend-empty-backfill"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('补全中')
    expect(wrapper.find('[data-testid="line-chart"]').exists()).toBe(false)
  })

  it('面板内下拉切换标的经 selectInstrument 入口生效（与入口带入同一选中事实源）', async () => {
    wireInvokeSeam({
      defaults: PANEL_DEFAULTS,
      overrides: {
        portfolio_value_trend: portfolioTrendResponse,
        instrument_price_trend: {
          instrument_id: 'inst-fund',
          points: [{ date: '2026-06-05', price_cents: 12850, currency_code: 'CNY' }],
        },
      },
    })
    // 入口带入 600000，再经面板下拉改选 000198
    const wrapper = mountWithEntry(stockInstrument)
    await flushPromises()
    expect(useInvestmentsSessionStore().trendInstrumentId).toBe('inst-1')
    componentVm(wrapper.findComponent('[data-testid="trend-instrument-select"]')).$emit(
      'update:value',
      'inst-fund',
    )
    await flushPromises()
    // 选中事实源与出图同步切到新标的
    expect(useInvestmentsSessionStore().trendInstrumentId).toBe('inst-fund')
    const call = mockInvoke.mock.calls.filter(([c]) => c === 'instrument_price_trend').at(-1)!
    expect((call[1] as { instrumentId: string }).instrumentId).toBe('inst-fund')
    expect(chartPayload(wrapper).datasets[0].data).toEqual([12850])
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

  it('曲线口径说明随模式切换概念：组合市值 / 单标的两条曲线各带说明（issue #1369）', async () => {
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    // 断言对准用户可观察结果：删掉口径说明接线即找不到触发器、本用例变红
    const trigger = wrapper.find('[data-testid="trend-concept-info"]')
    expect(trigger.exists()).toBe(true)
    expect(trigger.attributes('aria-label')).toBe('组合市值说明')
    // 组合曲线画的是历史市值：不含现金账户、跨币种用同期历史汇率（不是当期汇率）
    const tip = await hoverTipText(trigger)
    expect(tip).toContain('历史市值')
    expect(tip).toContain('同期历史汇率')
    wrapper.unmount()

    // 单标的：同一挂点换成该概念的口径（价格序列，不是持仓市值也不是收益率）；
    // 另起一次挂载而非切换模式——已开启的 tooltip 不跨挂载复用
    const instrumentWrapper = mountWithEntry(stockInstrument)
    await flushPromises()
    const instrumentTrigger = instrumentWrapper.find('[data-testid="trend-concept-info"]')
    expect(instrumentTrigger.attributes('aria-label')).toBe('单标的说明')
    expect(await hoverTipText(instrumentTrigger)).toContain('不是持仓市值')
    instrumentWrapper.unmount()
  })
})
