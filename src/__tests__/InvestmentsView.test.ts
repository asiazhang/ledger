import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { flushPromises } from '@vue/test-utils'
import { defineComponent, h, nextTick } from 'vue'
import InvestmentsView from '@/views/InvestmentsView.vue'
import { formatAmount } from '@ledger/money'
import { clickTab, findTab, probeColor } from '@ledger/test-support/dom'
import { componentVm } from '@ledger/test-support/component-vm'
import { mountWithDialog } from '@ledger/test-support/mount'
import { refCurrencies } from '@ledger/test-support/reference-stubs'
import { useAppStore } from '@/stores/app'
import { useInvestmentsSessionStore } from '@/stores/investments-session'
import { useWindowGuard } from '@/composables/useWindowGuard'
import { createOverlayToken, resetOverlays } from '@/composables/overlayRegistry'
import { clearViewResets, fireViewReset } from '@/composables/viewResetRegistry'
import { pnlSemanticColor } from '@/theme/semantic-colors'
import { makePnlSummary, mockHoldings } from './factories'
import {
  firePricesChanged,
  resetPricesChangedHandler,
} from './prices-changed-mock'
import type { Instrument } from '@ledger/types'

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

/** 视图挂载走共享基座（@ledger/test-support/mount.ts 单点收口）：顶层 InstrumentBrowser 调
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
    price_channel: 'quote',
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
    price_channel: 'quote',
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
    price_channel: 'quote',
  },
]

/** 投资域命令契约快照（标的列表/持仓/走势/盈亏汇总，均为静态空数据或固定行）。 */
const INVESTMENT_DEFAULTS = {
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
  // 持仓概览（issue #110）：盈亏 tab 顶部会拉取当前持仓
  list_holdings: [],
  // 累计收益聚合（issue #1077）：持仓概览同批拉取（全账本按币种分组）
  cumulative_pnl_summary: [{ currency_code: 'CNY', cumulative_pnl_cents: 45000 }],
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
  resetOverlays()
  clearViewResets()
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

  it('走势选中标的会话内保留（issue #1192）：切走页签再回来仍以同一标的查询出图', async () => {
    const wrapper = mountView()
    await nextTick()
    // 经标的列表入口进入单标的走势
    await clickTab(wrapper, '标的')
    await wrapper.find('[data-testid="view-trend-600000"]').trigger('click')
    await nextTick()
    await nextTick()
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'instrument_price_trend'),
    ).toHaveLength(1)
    expect(wrapper.get('[data-testid="line-chart"]').text()).toContain('600000 浦发银行')
    // 切到盈亏再回走势：页签重挂（非 KeepAlive），单标的选中经会话 store 恢复
    await clickTab(wrapper, '盈亏')
    await clickTab(wrapper, '走势')
    await flushPromises()
    const instCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'instrument_price_trend')
    expect(instCalls).toHaveLength(2)
    expect((instCalls.at(-1)![1] as { instrumentId: string }).instrumentId).toBe('inst-1')
    // 恢复的是选择不是快照：回到页签以同一标的现拉，仍出该标的的曲线
    expect(wrapper.get('[data-testid="line-chart"]').text()).toContain('600000 浦发银行')
  })
})

/** 持仓迁为投资页独立页签（issue #901）：持仓概览卡整体迁入新持仓页签，
 * 盈亏页签收窄为纯已实现盈亏视图；页签选中自 issue #1192 起随会话保留。 */
describe('InvestmentsView 持仓页签（issue #901）', () => {
  const cny = refCurrencies[0]

  it('页签点击经 store 意图入口落位（NTabs 受控回传，单一路径）', async () => {
    const wrapper = mountView()
    await flushPromises()
    expect(useInvestmentsSessionStore().activeTab).toBe('pnl')
    await clickTab(wrapper, '持仓')
    expect(useInvestmentsSessionStore().activeTab).toBe('holdings')
    expect(wrapper.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['持仓'])
  })

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

  it('持仓页签完整呈现：按币种合计统计（含累计收益）+ 持仓明细表 + 同步按钮在位', async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } })
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    expect(wrapper.text()).toContain('当前持仓')
    expect(wrapper.text()).toContain('总市值')
    expect(wrapper.text()).toContain(formatAmount(150000, cny))
    expect(wrapper.find('[data-testid="total-unrealized-pnl"]').text()).toBe(
      `持仓收益${formatAmount(30000, cny)}`,
    )
    // 累计收益卡（issue #1077）：持仓页签合计区新增、按币种分组展示
    expect(wrapper.find('[data-testid="total-cumulative-pnl"]').text()).toBe(
      `累计收益${formatAmount(45000, cny)}`,
    )
    expect(wrapper.text()).not.toContain('未实现盈亏')
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
    // 盈亏页收敛后只剩筛选 + 按年/按账户两表（ADR-0107 修订注记，2026-09-13）
    expect(wrapper.text()).toContain('按年度汇总')
    expect(wrapper.text()).toContain('按账户汇总')
    expect(wrapper.text()).not.toContain('已实现盈亏概览')
    expect(wrapper.text()).not.toContain('按标的汇总')
    await clickTab(wrapper, '持仓')
    await clickTab(wrapper, '盈亏')
    expect(wrapper.text()).not.toContain('当前持仓')
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(false)
  })

  it('盈亏页两表的已实现盈亏数字着盈亏涨跌色（红涨绿跌，随主题取变体）', async () => {
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: {
        realized_pnl_summary: makePnlSummary({
          by_year: [
            { year: '2026', currency_code: 'CNY', realized_pnl_cents: 30000 },
            { year: '2025', currency_code: 'CNY', realized_pnl_cents: -12345 },
          ],
          by_account: [
            {
              account_id: 'acc-1',
              account_name: '证券账户A',
              currency_code: 'CNY',
              realized_pnl_cents: -12345,
            },
          ],
        }),
      },
    })
    const wrapper = mountView()
    await flushPromises()
    const theme = useAppStore().theme
    // 两表同一列口径：盈亏数字逐行按自身符号取色（与持仓页「持仓收益」列、合计三卡同源）
    const colors = wrapper
      .findAll('td[data-col-key="realized_pnl_cents"] span')
      .map((s) => (s.element as HTMLElement).style.color)
    expect(colors).toEqual([
      probeColor(pnlSemanticColor(30000, theme)),
      probeColor(pnlSemanticColor(-12345, theme)),
      probeColor(pnlSemanticColor(-12345, theme)),
    ])
    // 文本口径不变：仍按行币种走 formatAmount
    expect(wrapper.findAll('td[data-col-key="realized_pnl_cents"]').map((c) => c.text())).toEqual([
      formatAmount(30000, cny),
      formatAmount(-12345, cny),
      formatAmount(-12345, cny),
    ])
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

  it('页签选中会话内保留（issue #1192）：卸载重挂回离开时的持仓页签', async () => {
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    expect(wrapper.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['持仓'])
    wrapper.unmount()
    const remounted = mountView()
    await flushPromises()
    expect(remounted.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['持仓'])
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

/**
 * 持仓页签筛选/排序/页码会话内保留（issue #1192 / ADR-0094）：页签内容仍为
 * display-directive 'if' 重挂（显式否决 KeepAlive），保留由投资页会话状态 store
 * 承担——切走再切回恢复离开时的搜索/账户过滤/排序/页码，冷启动（新 pinia）回
 * 默认，全程零写盘。删除保留接线（状态退回实例级瞬态）即下方断言变红。
 */
describe('InvestmentsView 持仓页签会话内保留（issue #1192）', () => {
  /** 搜索输入 → 应用值：防抖窗口推进 + 微任务清空（HoldingsOverview 同款桥） */
  async function typeSearch(wrapper: ReturnType<typeof mountView>, text: string) {
    await wrapper.find('[data-testid="holdings-search"] input').setValue(text)
    vi.advanceTimersByTime(300)
    await flushPromises()
  }

  /** 市值列头点击：首次点击落降序（受控排序形态见 HoldingsOverview 测试） */
  async function clickMarketValueHeader(wrapper: ReturnType<typeof mountView>) {
    await wrapper.findAll('th').find((th) => th.text() === '市值')!.trigger('click')
    await flushPromises()
  }

  const symbols = (wrapper: ReturnType<typeof mountView>) =>
    wrapper.findAll('td[data-col-key="symbol"]').map((c) => c.text())

  /** 25 行持仓夹具：第二页余 5 行（页码保留的观察面；行键唯一避免表格复用） */
  const manyHoldings = Array.from({ length: 25 }, (_, i) => ({
    ...mockHoldings[i % mockHoldings.length]!,
    id: `ph-${i}`,
    instrument_id: mockHoldings[i % mockHoldings.length]!.instrument_id,
  }))

  beforeEach(() => {
    vi.useFakeTimers()
    mockRoute.query = {}
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('切走再切回持仓页签：搜索/排序仍在，行集按恢复的选择现过滤', async () => {
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    await flushPromises()
    // 排序须先于搜索设置：行集合收窄为空态后列头不再可点
    await clickMarketValueHeader(wrapper)
    expect(symbols(wrapper)).toEqual(['600000', '000001'])
    await typeSearch(wrapper, '600')
    expect(symbols(wrapper)).toEqual(['600000'])
    // 切到标的再回持仓：页签重挂，筛选/排序经会话 store 恢复（任一未保留即变红：
    // 搜索丢失 → 两个代码行；排序丢失 → 000001 在首）
    await clickTab(wrapper, '标的')
    await clickTab(wrapper, '持仓')
    await flushPromises()
    expect(symbols(wrapper)).toEqual(['600000'])
  })

  it('切走再切回持仓页签：账户过滤与页码仍在（25 行夹具第二页恢复）', async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: manyHoldings } })
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    await flushPromises()
    // 账户过滤收窄（参考夹具的投资账户 acc-1；收窄后仍超一页）
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      'update:value',
      'acc-1',
    )
    await flushPromises()
    await wrapper
      .findAll('.n-pagination-item')
      .find((el) => el.text() === '2')!
      .trigger('click')
    await flushPromises()
    const pageTwoRows = wrapper.findAll('td[data-col-key="symbol"]').length
    expect(pageTwoRows).toBeLessThan(20)
    await clickTab(wrapper, '标的')
    await clickTab(wrapper, '持仓')
    await flushPromises()
    // 恢复账户过滤 + 第 2 页：行数与分页条当前页都留在离开时的位置
    expect(wrapper.findAll('td[data-col-key="symbol"]').length).toBe(pageTwoRows)
    expect(wrapper.findAll('.n-pagination-item--active').map((el) => el.text())).toEqual(['2'])
  })

  it('新 pinia 表达冷启动：页签与持仓筛选回默认，且全程零写盘', async () => {
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    await flushPromises()
    await clickMarketValueHeader(wrapper)
    await typeSearch(wrapper, '600')
    expect(symbols(wrapper)).toEqual(['600000'])
    const keysBefore = Object.keys(localStorage)
    wrapper.unmount()

    // 新 pinia = 新会话（应用重启）：默认页签盈亏、持仓无筛选
    // （store 复位断言独立于视图渲染：同时钉住「恢复的选择不是快照」与冷启动口径）
    setActivePinia(createPinia())
    const coldStore = useInvestmentsSessionStore()
    expect(coldStore.activeTab).toBe('pnl')
    expect(coldStore.holdingsSorter).toBeNull()
    expect(coldStore.holdingsSearch).toBe('')
    expect(coldStore.holdingsPage).toBe(1)
    const second = mountView()
    await flushPromises()
    expect(second.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['盈亏'])
    await clickTab(second, '持仓')
    await flushPromises()
    // 默认代码字母序（非离开时的市值降序）：'000001' < '600000'
    expect(symbols(second)).toEqual(['000001', '600000'])
    // 会话内保留零持久化：全程 localStorage 零写入
    expect(Object.keys(localStorage)).toEqual(keysBefore)
  })

  it('越界页码回落到有效范围并写回保留态：行集回增多页也不凭空跳页（ADR-0094 决策 3）', async () => {
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: manyHoldings } })
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    await flushPromises()
    await wrapper
      .findAll('.n-pagination-item')
      .find((el) => el.text() === '2')!
      .trigger('click')
    await flushPromises()
    expect(useInvestmentsSessionStore().holdingsPage).toBe(2)
    await clickTab(wrapper, '标的')
    await clickTab(wrapper, '持仓')
    await flushPromises()
    // 离开期间持仓缩到 1 页（回访读页码即回落）
    wireInvokeSeam({
      defaults: INVESTMENT_DEFAULTS,
      overrides: { list_holdings: manyHoldings.slice(0, 5) },
    })
    await clickTab(wrapper, '标的')
    await clickTab(wrapper, '持仓')
    await flushPromises()
    expect(wrapper.findAll('td[data-col-key="symbol"]')).toHaveLength(5)
    // 保留态一并回落（非只钳展示）：否则行集回增后旧页码重新生效
    expect(useInvestmentsSessionStore().holdingsPage).toBe(1)
    // 行集回增到多页：仍是第 1 页，不跳回离开时的第 2 页
    wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: manyHoldings } })
    await clickTab(wrapper, '标的')
    await clickTab(wrapper, '持仓')
    await flushPromises()
    expect(useInvestmentsSessionStore().holdingsPage).toBe(1)
    expect(wrapper.findAll('.n-pagination-item--active').map((el) => el.text())).toEqual(['1'])
  })

  it('防抖窗口内离开页签：在途输入不落地（撤销定时器），回显回到已应用值', async () => {
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '持仓')
    await flushPromises()
    // 先应用一个搜索（保留态基线）
    await typeSearch(wrapper, '600')
    expect(symbols(wrapper)).toEqual(['600000'])
    // 输入新值但不等防抖窗口，立刻切走页签（页签重挂 = 作用域销毁）
    await wrapper.find('[data-testid="holdings-search"] input').setValue('000001')
    await clickTab(wrapper, '标的')
    // 防抖窗口推进：未应用的输入不得落地
    vi.advanceTimersByTime(300)
    await flushPromises()
    const store = useInvestmentsSessionStore()
    expect(store.holdingsSearch).toBe('600')
    expect(store.holdingsSearchInput).toBe('600')
    // 回持仓页签：仍是已应用的搜索与命中行（不是未落地的输入）
    await clickTab(wrapper, '持仓')
    await flushPromises()
    expect(symbols(wrapper)).toEqual(['600000'])
  })
})

/**
 * ESC 复位接线（ADR-0094 决策 4）：投资视图向复位回调注册表声明 store 的
 * resetToDefault，无弹层 ESC 经窗口行为守卫消费——复位即清除保留态本身
 * （复位后离开再回来 = 默认），有弹层时不叠加动作。删除复位接线即下方断言变红。
 */
describe('InvestmentsView ESC 复位（issue #1192）', () => {
  /** 窗口行为守卫宿主（App.vue 同构：守卫全局唯一） */
  function mountGuardHost() {
    const Host = defineComponent({
      setup() {
        useWindowGuard()
        return () => h('div')
      },
    })
    return mountWithDialog(Host)
  }

  function fireEscape() {
    document.body.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }),
    )
  }

  const symbols = (wrapper: ReturnType<typeof mountView>) =>
    wrapper.findAll('td[data-col-key="symbol"]').map((c) => c.text())

  it('无弹层 ESC：持仓筛选/排序清零回默认（清除保留态本身）', async () => {
    vi.useFakeTimers()
    try {
      wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } })
      const guard = mountGuardHost()
      await flushPromises()
      const wrapper = mountView()
      await flushPromises()
      await clickTab(wrapper, '持仓')
      await flushPromises()
      await wrapper.findAll('th').find((th) => th.text() === '市值')!.trigger('click')
      await flushPromises()
      await wrapper.find('[data-testid="holdings-search"] input').setValue('600')
      vi.advanceTimersByTime(300)
      await flushPromises()
      expect(symbols(wrapper)).toEqual(['600000'])

      fireEscape()
      await flushPromises()
      // 复位即清除保留态本身（页签回默认 + 筛选/排序/页码清零）：过滤残留会让
      // 「回到默认全量列表」不成立、排序残留会让默认代码序不成立
      expect(useInvestmentsSessionStore().holdingsSorter).toBeNull()
      expect(useInvestmentsSessionStore().holdingsSearch).toBe('')
      expect(useInvestmentsSessionStore().holdingsPage).toBe(1)
      expect(wrapper.findAll('.n-tabs-tab--active').map((el) => el.text())).toEqual(['盈亏'])
      await clickTab(wrapper, '持仓')
      await flushPromises()
      expect(symbols(wrapper)).toEqual(['000001', '600000'])
      wrapper.unmount()
      guard.unmount()
    } finally {
      vi.useRealTimers()
    }
  })

  it('无弹层 ESC：走势选中标的清除、回到默认组合曲线（复位后重进仍是组合模式）', async () => {
    const guard = mountGuardHost()
    await flushPromises()
    const wrapper = mountView()
    await flushPromises()
    await clickTab(wrapper, '标的')
    await wrapper.find('[data-testid="view-trend-600000"]').trigger('click')
    await flushPromises()
    expect(wrapper.get('[data-testid="line-chart"]').text()).toContain('600000 浦发银行')
    const instrumentCallsAfterEntry = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === 'instrument_price_trend',
    ).length
    expect(instrumentCallsAfterEntry).toBe(1)

    fireEscape()
    await flushPromises()
    // 复位即清除保留态：选中标的从会话态消失（复位后离开再回来 = 默认组合曲线）
    expect(useInvestmentsSessionStore().trendInstrumentId).toBeNull()
    await clickTab(wrapper, '盈亏')
    await clickTab(wrapper, '走势')
    await flushPromises()
    // 组合走势空数据 → 引导文案（而非上一标的的单标的曲线残留）
    expect(wrapper.text()).toContain('暂无历史价格数据')
    // 复位的保留态已清除：没有新的单标的查询发生（残留会以恢复的标的重拉现拉）
    expect(
      mockInvoke.mock.calls.filter(([cmd]) => cmd === 'instrument_price_trend'),
    ).toHaveLength(instrumentCallsAfterEntry)
    wrapper.unmount()
    guard.unmount()
  })

  it('有弹层时 ESC 不复位：弹层库默认关闭行为接管，保留态不动', async () => {
    vi.useFakeTimers()
    try {
      wireInvokeSeam({ defaults: INVESTMENT_DEFAULTS, overrides: { list_holdings: mockHoldings } })
      const guard = mountGuardHost()
      await flushPromises()
      const wrapper = mountView()
      await flushPromises()
      await clickTab(wrapper, '持仓')
      await flushPromises()
      await wrapper.find('[data-testid="holdings-search"] input').setValue('600')
      vi.advanceTimersByTime(300)
      await flushPromises()
      const token = createOverlayToken('modal')
      token.set(true)
      fireEscape()
      await flushPromises()
      // 保留态不动（一次按键只做一件事：关弹层）
      expect(symbols(wrapper)).toEqual(['600000'])
      token.set(false)
      wrapper.unmount()
      guard.unmount()
    } finally {
      vi.useRealTimers()
    }
  })

  it('视图卸载即撤销复位注册：离开投资页后守卫消费不到本视图的保留态', async () => {
    const wrapper = mountView()
    await flushPromises()
    expect(fireViewReset()).toBe(true)
    wrapper.unmount()
    expect(fireViewReset()).toBe(false)
  })
})
