import { describe, it, expect, vi, beforeEach } from 'vitest'
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { h, nextTick } from 'vue'
import { NDialogProvider } from 'naive-ui'
import InvestmentsView from '@/views/InvestmentsView.vue'
import type { Instrument } from '@/types'

// 走势图用共享桩组件替代：组件层测试只验证数据联动与文案渲染，不验证 canvas 绘制
vi.mock('vue-chartjs', async () => {
  const { LineChartStub } = await import('./line-chart-stub')
  return { Line: LineChartStub }
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
  // 参考 store 预载走接缝 opt-in 参数（五个 list 命令由桩层规范夹具兑底）。
  await wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, refreshReferenceStores: true }).ready
})

describe('InvestmentsView 标的 tab', () => {
  /** 标的页 InstrumentBrowser 顶层调用 useAppDialog（删除二次确认，issue #292），
   * 与 App.vue 同构需 NDialogProvider 包裹（先例：AccountsView.test.ts 的 mountView）。 */
  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(InvestmentsView) },
    })
  }

  // issue #769：页签存在性收行——行 = 页签名，删页签即红（生杀线内，见 CONTEXT-testing「存在性断言」）。
  it.each(['盈亏', '标的', '走势'])('%s tab 存在', async (tab) => {
    const wrapper = mountView()
    await nextTick()
    expect(wrapper.findAll('.n-tabs-tab').map((t) => t.text())).toContain(tab)
  })

  // issue #769：标的 tab 内容存在性收行——原「显示标的搜索框」与「支持搜索」近重复合并为一行
  // （「搜索」是「搜索代码或名称」的子串），另收原「市场筛选」一行。
  it.each(['搜索代码或名称', '全部市场'])('标的 tab 显示「%s」', async (text) => {
    const wrapper = mountView()
    await nextTick()
    await wrapper.findAll('.n-tabs-tab')[1].trigger('click')
    await nextTick()
    await nextTick()
    expect(wrapper.html()).toContain(text)
  })

  it('标的 tab 分页请求携带 page/page_size', async () => {
    const wrapper = mountView()
    await nextTick()
    const instTab = wrapper.findAll('.n-tabs-tab')[1]
    await instTab.trigger('click')
    await nextTick()
    await nextTick()
    expect(lastInvokeArgs('list_instruments').filter).toMatchObject({ page: 1, page_size: 50 })
  })

  it('标的列表「走势」入口：切到走势 tab 并以单标的模式查询该标的', async () => {
    const wrapper = mountView()
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
    const wrapper = mountView()
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

  function mountView() {
    return mount(NDialogProvider, {
      slots: { default: () => h(InvestmentsView) },
    })
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
