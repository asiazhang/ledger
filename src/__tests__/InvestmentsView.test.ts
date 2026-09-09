import { describe, it, expect, vi, beforeEach } from 'vitest'
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import InvestmentsView from '@/views/InvestmentsView.vue'
import { formatAmount } from '@/utils/money'
import { clickTab, findTab } from './helpers/dom'
import { mountWithDialog } from './helpers/mount'
import { refCurrencies } from './helpers/reference-stubs'
import { mockHoldings } from './factories'
import {
  firePricesChanged,
  resetPricesChangedHandler,
} from './prices-changed-mock'
import type { Instrument } from '@/types'

// 走势图用共享桩组件替代：组件层测试只验证数据联动与文案渲染，不验证 canvas 绘制
vi.mock('vue-chartjs', async () => {
  const { LineChartStub } = await import('./line-chart-stub')
  return { Line: LineChartStub }
})

// 价格失效信号订阅 mock（同 HoldingsOverview.test.ts 基座）：捕获订阅回调，
// 视图级用例手动触发模拟后端 emit（同步写价 → 持仓自动刷新，issue #901）。
vi.mock('@/composables/usePricesChanged', async () => {
  const { capturePricesChangedHandler } = await import('./prices-changed-mock')
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  }
})

// focus 参数读取自路由 query（useFocusParam 注入 getter，spec #704 / issue #709）。
// 可控 mockRoute 替代真实 router（ItemsView 落点测试同款）：默认空 query（无 focus
// 空转），来源落点场景在 mount 前写入 focus。
const pushMock = vi.fn()
const mockRoute = { query: {} as Record<string, string> }
vi.mock('vue-router', () => ({
  useRoute: () => mockRoute,
  useRouter: () => ({ push: pushMock }),
}))

/** 视图挂载走共享基座（helpers/mount.ts 单点收口）：顶层 InstrumentBrowser 调
 * useAppDialog（删除二次确认，issue #292），与 App.vue 同构需 NDialogProvider 包裹。 */
const mountView = () => mountWithDialog(InvestmentsView)

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
    source: 'eastmoney',
    price_cents: null,
    invested: false,
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
    source: 'eastmoney',
    price_cents: null,
    invested: false,
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
    source: 'eastmoney',
    price_cents: null,
    invested: false,
  },
]

/** 投资域命令契约快照（标的列表/持仓/走势/盈亏汇总，均为静态空数据或固定行）。 */
const INVESTMENT_DEFAULTS = {
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
  // 持仓概览（issue #110）：盈亏 tab 顶部会拉取当前持仓
  list_holdings: [],
  // 走势（issue #139）：标的列表「走势」入口切入走势 tab 时由面板拉取
  portfolio_value_trend: { currency_code: 'CNY', points: [] },
  instrument_price_trend: {
    instrument_id: 'inst-1',
    points: [{ date: '2026-06-05', price_cents: 1500, currency_code: 'CNY' }],
  },
  realized_pnl_summary: {
    total_realized_pnl_cents: 0,
    by_year: [],
    by_account: [],
    by_instrument: [],
    details: [],
  },
}

beforeEach(async () => {
  resetPricesChangedHandler()
  // 参考 store 预载走接缝 opt-in 参数（五个 list 命令由桩层规范夹具兑底）。
  await wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, refreshReferenceStores: true }).ready
})

describe('InvestmentsView 标的 tab', () => {
  // issue #769：页签存在性收行——行 = 页签名，删页签即红（生杀线内，见 CONTEXT-testing「存在性断言」）。
  it.each(['盈亏', '持仓', '标的', '走势'])('%s tab 存在', async (tab) => {
    const wrapper = mountView()
    await nextTick()
    expect(findTab(wrapper, tab, { exact: true }), `页签「${tab}」应存在`).toBeTruthy()
  })

  // issue #769：标的 tab 内容存在性收行——原「显示标的搜索框」与「支持搜索」近重复合并为一行
  // （「搜索」是「搜索代码或名称」的子串），另收原「市场筛选」一行。
  it.each(['搜索代码或名称', '全部市场'])('标的 tab 显示「%s」', async (text) => {
    const wrapper = mountView()
    await nextTick()
    await clickTab(wrapper, '标的')
    expect(wrapper.html()).toContain(text)
  })

  it('标的 tab 分页请求携带 page/page_size', async () => {
    const wrapper = mountView()
    await nextTick()
    await clickTab(wrapper, '标的')
    expect(lastInvokeArgs('list_instruments').filter).toMatchObject({ page: 1, page_size: 50 })
  })

  it('标的列表「走势」入口：切到走势 tab 并以单标的模式查询该标的', async () => {
    const wrapper = mountView()
    await nextTick()
    // 进入标的 tab
    await clickTab(wrapper, '标的')
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
    const wrapper = mountView()
    await nextTick()
    // 经标的列表入口进入单标的走势
    await clickTab(wrapper, '标的')
    await wrapper.find('[data-testid="view-trend-600000"]').trigger('click')
    await nextTick()
    await nextTick()
    const instCallsAfterEntry = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === 'instrument_price_trend',
    ).length
    expect(instCallsAfterEntry).toBe(1)
    // 切到盈亏再直入走势：入口残留已清空，回到组合模式（无新的单标的查询）
    await clickTab(wrapper, '盈亏')
    await clickTab(wrapper, '走势')
    const instCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'instrument_price_trend')
    expect(instCalls.length).toBe(1)
    // 组合走势空数据 → 引导文案（而非上一标的的单标的曲线）
    expect(wrapper.text()).toContain('暂无历史价格数据')
  })
})

/** 持仓迁为投资页独立页签（issue #901）：持仓概览卡整体迁入新持仓页签，
 * 盈亏页签收窄为纯已实现盈亏视图；页签选中维持组件本地瞬态。 */
describe('InvestmentsView 持仓页签（issue #901）', () => {
  const cny = refCurrencies[0]

  it('页签顺序为盈亏/持仓/标的/走势，默认选中盈亏', async () => {
    const wrapper = mountView()
    await flushPromises()
    expect(wrapper.findAll('.n-tabs-tab').map((el) => el.text())).toEqual([
      '盈亏',
      '持仓',
      '标的',
      '走势',
    ])
    expect(wrapper.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['盈亏'])
  })

  it('持仓页签完整呈现：按币种合计统计 + 持仓明细表 + 同步按钮在位', async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } })
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    expect(wrapper.text()).toContain('当前持仓')
    expect(wrapper.text()).toContain('总市值')
    expect(wrapper.text()).toContain(formatAmount(150000, cny))
    expect(wrapper.text()).toContain('未实现盈亏合计')
    // 持仓明细行上屏（600000 浦发银行）
    expect(wrapper.text()).toContain('600000')
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(true)
  })

  it('盈亏页签不再渲染持仓卡（收窄为纯已实现盈亏视图），访问持仓页签后返回亦不残留', async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } })
    const wrapper = mountView()
    await flushPromises()
    expect(wrapper.text()).not.toContain('当前持仓')
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false)
    // 盈亏页自有过滤与汇总视图原样保留
    expect(wrapper.text()).toContain('已实现盈亏概览')
    await clickTab(wrapper, '持仓')
    await clickTab(wrapper, '盈亏')
    expect(wrapper.text()).not.toContain('当前持仓')
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false)
  })

  it('价格失效信号触发持仓重查：翻新后的市值合计上屏（自动刷新贯通取数与渲染）', async () => {
    // 第二次取数返回写价后的行情：h-1 价格/市值/未实现盈亏联动翻新（合计 150000 → 300000）
    const repriced = [
      {
        ...mockHoldings[0],
        latest_price_cents: 300000,
        market_value_cents: 300000,
        unrealized_pnl_cents: 180000,
      },
      mockHoldings[1],
    ]
    let holdingsCalls = 0
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: { list_holdings: () => (holdingsCalls++ === 0 ? mockHoldings : repriced) },
    })
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    expect(wrapper.text()).toContain(formatAmount(150000, cny))
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length
    firePricesChanged()
    await flushPromises()
    // 重查恰好一次，且翻新数据上屏（旧合计不再残留）
    expect(mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length).toBe(callsBefore + 1)
    expect(wrapper.text()).toContain(formatAmount(300000, cny))
    expect(wrapper.text()).not.toContain(formatAmount(150000, cny))
  })

  it('页签选中为瞬态：卸载重挂回默认盈亏（不入 URL、不持久化）', async () => {
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    expect(wrapper.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['持仓'])
    wrapper.unmount()
    const remounted = mountView()
    await flushPromises()
    expect(remounted.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['盈亏'])
  })
})

/** 来源跳转落点（spec #704 / issue #709，词汇表「实体定位参数（focus 参数）」）：
 * 视图装配断言——focus 在场 → 切走势页签 + 按 id 解析标的 + 单标的模式查询；
 * 无 focus 空转；读一次语义下消费后 query 变化不再消费；解析失败停留组合走势
 * （不提供落空的跳转）。清仓标的（invested=false）照常可达（走势不依赖持仓）。 */
describe('InvestmentsView 来源跳转落点（issue #709）', () => {
  const focusInstrument: Instrument = {
    ...mockInstruments[0],
    id: 'inst-sell',
    symbol: '600519',
    name: '招商银行',
    invested: false,
  }

  function activeTabText(wrapper: ReturnType<typeof mountView>): string {
    return wrapper.findAll('.n-tabs-tab--active').map((t) => t.text()).join()
  }

  beforeEach(async () => {
    mockRoute.query = {}
    // focus 落点用例接 get_instrument（#709 新增按 id 精确取标的）：命中返回
    // 清仓标的投影，未命中按后端同款码化错误拒绝
    await wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: {
        get_instrument: (args) =>
          args?.id === 'inst-sell'
            ? focusInstrument
            : Promise.reject(new Error('标的 xxx 不存在')),
      },
      refreshReferenceStores: true,
    }).ready
  })

  it('focus 在场：切到走势页签，按 id 精确取标的并以单标的模式查询该标的（清仓标的照常可达）', async () => {
    mockRoute.query = { focus: 'inst-sell' }
    const wrapper = mountView()
    await flushPromises()

    expect(lastInvokeArgs('get_instrument')).toMatchObject({ id: 'inst-sell' })
    expect(activeTabText(wrapper)).toContain('走势')
    const call = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'instrument_price_trend').at(-1)
    expect(call).toBeTruthy()
    expect((call![1] as { instrumentId: string }).instrumentId).toBe('inst-sell')
    // 选中态上屏：单标的曲线 dataset 标签 = 代码 + 名称（走势页签选中该标的，
    // 演示路径「卖出交易点击 → 走势页签选中」的装配锚点）
    expect(wrapper.get('[data-testid="line-chart"]').text()).toContain('600519 招商银行')
  })

  it('无 focus：停留默认盈亏页签，不调按 id 取标的', async () => {
    const wrapper = mountView()
    await flushPromises()

    expect(activeTabText(wrapper)).toContain('盈亏')
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'get_instrument')).toBe(false)
  })

  it('读一次语义：消费后 query 再变（页签切换 replace 保留残留 focus 场景）不再消费', async () => {
    mockRoute.query = { focus: 'inst-sell' }
    mountView()
    await flushPromises()
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'get_instrument').length).toBe(1)

    mockRoute.query = { tab: 'investments', focus: 'inst-other' }
    await flushPromises()
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'get_instrument').length).toBe(1)
  })

  it('focus 解析失败（无效 id）：停留走势页签组合模式（不提供落空的跳转）', async () => {
    mockRoute.query = { focus: 'ghost' }
    const wrapper = mountView()
    await flushPromises()

    expect(activeTabText(wrapper)).toContain('走势')
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'instrument_price_trend')).toBe(false)
  })
})
