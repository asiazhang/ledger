import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import type { VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import type { Chart, ChartOptions, TooltipItem } from 'chart.js'
import ReportsView from '@/views/ReportsView.vue'
import PortfolioTrendPanel from '@/components/investments/PortfolioTrendPanel.vue'
import SubscriptionSpendPanel from '@/components/scheduled/SubscriptionSpendPanel.vue'
import { amountPrivacyEnabled } from '@/utils/money'
import { useReferenceStore } from '@/stores/reference'
import { invokeHandler, makeCategory, makeInstrument } from './factories'
import type {
  CategoryShare,
  Instrument,
  MerchantSharesReport,
  MonthlySummary,
  PortfolioValueTrend,
  ReportDateRange,
  SubscriptionSpendOverview,
} from '@/types'

// 图表数字同源掩码核查（issue #567，spec #564 user story 4）：逐面断言
// 「轴刻度 / 图内标注 / tooltip」三类渲染点与列表数字同一掩码口径——
// 开启时恒 `••••`、百分比保留、关闭时与现状逐字符一致（回归保障）。
// jsdom 无 canvas：图桩承接（line-chart-stub 先例），经 props 拿到真
// options/plugins 对象直接调用回调（JSON 序列化桩会丢函数，不可用于本面）。

vi.mock('vue-chartjs', async () => {
  const { BarChartStubWithOptions, LineChartStub } = await import('./line-chart-stub')
  return { Bar: BarChartStubWithOptions, Line: LineChartStub }
})

// ReportsView 跳转下钻经 useRouter；本文件不涉及跳转，仅满足装配
const pushMock = vi.fn()
vi.mock('vue-router', () => ({ useRouter: () => ({ push: pushMock }) }))

const mockInvoke = vi.mocked(invoke)

// 固定「今天」= 2026-01-15：报表页默认「当年」快照随之确定（ReportsView 测试同款前提）
const Y = 2026

const cny = { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 }

const mockRange: ReportDateRange = { min_date: '2020-03-01', max_date: '2027-11-30' }

const mockCategories = [
  makeCategory({ id: 'food', name: '餐饮', sort_order: 0 }),
  makeCategory({ id: 'food-snack', name: '零食', parent_id: 'food', sort_order: 1 }),
  makeCategory({ id: 'transport', name: '交通', sort_order: 2 }),
]

const mockShares: CategoryShare[] = [
  { category_id: 'food', category_name: '餐饮', amount_cents: 5000 },
  { category_id: 'transport', category_name: '交通', amount_cents: 3000 },
  { category_id: 'food-snack', category_name: '零食', amount_cents: 1000 },
  { category_id: '', category_name: '未分类', amount_cents: 800 },
]

const mockMonthly: MonthlySummary[] = [
  { month: `${Y}-01`, income_cents: 100000, expense_cents: 1234560, refund_cents: 5000 },
]

const mockMerchants: MerchantSharesReport = {
  rows: [
    { merchant_id: 'm-1', merchant_name: '超市', amount_cents: 50000, transaction_count: 4 },
    { merchant_id: 'm-2', merchant_name: '咖啡', amount_cents: 9900, transaction_count: 2 },
  ],
  // 全量合计刻意 ≠ rows 合计（59900），供占比分母断言识别真源（issue #588）
  total_cents: 150000,
}

const portfolioTrend: PortfolioValueTrend = {
  currency_code: 'CNY',
  points: [
    { date: '2026-06-05', market_value_cents: 100000 },
    { date: '2026-06-12', market_value_cents: 110000 },
  ],
}

const instrumentTrend = {
  instrument_id: 'inst-1',
  points: [{ date: '2026-06-05', price_cents: 1500, currency_code: 'CNY' }],
}

const stockInstrument: Instrument = makeInstrument({ id: 'inst-1' })

const spendOverview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 3000,
  this_year_native_cents: 64800,
  projected_month_native_cents: 4030,
  projected_year_native_cents: 48360,
  months: [
    { month: '2025-04', native_cents: 0 },
    { month: '2026-03', native_cents: 34800 },
  ],
  rows: [],
}

function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: [cny],
        list_accounts: [],
        list_categories: mockCategories,
        list_merchants: [],
        list_insurers: [],
        report_date_range: mockRange,
        monthly_summary: mockMonthly,
        category_shares: mockShares,
        merchant_shares: mockMerchants,
        list_holdings: [],
        list_instruments: { items: [stockInstrument], total: 1 },
        portfolio_value_trend: portfolioTrend,
        instrument_price_trend: instrumentTrend,
        subscription_spend_overview: spendOverview,
      },
      extra,
    ),
  )
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  pushMock.mockReset()
  baseInvoke()
  amountPrivacyEnabled.value = false
  vi.useFakeTimers()
  vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0))
  await useReferenceStore().refresh()
})

afterEach(() => {
  amountPrivacyEnabled.value = false
  vi.useRealTimers()
})

enableAutoUnmount(afterEach)

/** 第 index 个 Bar 图桩（ReportsView 模板序：0 = 月度收支、1 = 分类构成）的 prop */
function barProp(wrapper: VueWrapper, index: number, prop: 'options' | 'plugins'): unknown {
  const bar = wrapper.findAllComponents({ name: 'Bar' })[index]
  expect(bar).toBeTruthy()
  return bar!.props(prop)
}

function barOptions(wrapper: VueWrapper, index: number): ChartOptions<'bar'> {
  return barProp(wrapper, index, 'options') as ChartOptions<'bar'>
}

/** 线性轴刻度 callback（y 轴或 x 轴值轴），缺失即失败 */
function linearTick(
  options: ChartOptions<'bar' | 'line'>,
  axis: 'x' | 'y',
): (value: number | string, index: number, ticks: unknown[]) => string {
  const cb = options.scales?.[axis]?.ticks?.callback
  expect(typeof cb).toBe('function')
  return cb as (value: number | string, index: number, ticks: unknown[]) => string
}

/** tooltip 文本回调（label / afterBody），缺失即失败 */
function tooltipCb<O>(options: ChartOptions<'bar' | 'line'>, key: 'label' | 'afterBody'): O {
  const cb = options.plugins?.tooltip?.callbacks?.[key]
  expect(typeof cb).toBe('function')
  return cb as O
}

function tooltipItem(raw: number, datasetLabel = ''): TooltipItem<'bar'> {
  return { dataset: { label: datasetLabel }, raw } as unknown as TooltipItem<'bar'>
}

/** 月度图 afterBody 入参：三 dataset（收入/支出/退款）悬停项，净额 = 收入−支出+退款 */
function monthlyTooltipItems(): TooltipItem<'bar'>[] {
  return [
    { datasetIndex: 0, raw: 100000 },
    { datasetIndex: 1, raw: 1234560 },
    { datasetIndex: 2, raw: 5000 },
  ] as unknown as TooltipItem<'bar'>[]
}

/** barEndAmounts 插件的假图：捕获 fillText 调用（jsdom 无 2D context，绘制即此调用） */
function fakeBarEndChart(values: number[]) {
  const fillText = vi.fn()
  const chart = {
    data: { datasets: [{ data: values }] },
    options: { color: '#ffffff' },
    ctx: {
      save: vi.fn(),
      restore: vi.fn(),
      fillStyle: '',
      font: '',
      textBaseline: '',
      textAlign: '',
      fillText,
    },
    getDatasetMeta: () => ({
      data: values.map(() => ({ getProps: () => ({ x: 100, y: 20 }) })),
    }),
  }
  return { chart: chart as unknown as Chart<'bar'>, fillText }
}

describe('报表卡面：月度收支（轴刻度 / tooltip 同源掩码，issue #567）', () => {
  it('y 轴刻度与 tooltip（含净额 afterBody）关闭态与现状逐字符一致', async () => {
    const wrapper = mount(ReportsView)
    await flushPromises()
    const options = barOptions(wrapper, 0)
    const tick = linearTick(options, 'y')
    // zh-CN 四位分组：12345.6 → 1,2345.6；1000 → 1000
    expect(tick(1234560, 0, [])).toBe('1,2345.6')
    expect(tick(100000, 0, [])).toBe('1000')
    const label = tooltipCb<(item: TooltipItem<'bar'>) => string>(options, 'label')
    expect(label(tooltipItem(100000, '收入'))).toBe('收入: 1000')
    const afterBody = tooltipCb<(items: TooltipItem<'bar'>[]) => string>(options, 'afterBody')
    // 净额 = 100000 − 1234560 + 5000 = −1129560 分 → −1,1295.6
    expect(afterBody(monthlyTooltipItems())).toBe('净额: -1,1295.6')
  })

  it('开启后 y 轴刻度与 tooltip 恒掩码；options 随开关重算驱动重绘（即时生效）', async () => {
    const wrapper = mount(ReportsView)
    await flushPromises()
    const optionsOff = barOptions(wrapper, 0)
    expect(linearTick(optionsOff, 'y')(1234560, 0, [])).toBe('1,2345.6')

    amountPrivacyEnabled.value = true
    await nextTick()

    // 接缝同源：同一回调（闭包读格式化函数）即时翻转
    expect(linearTick(optionsOff, 'y')(1234560, 0, [])).toBe('••••')
    // 重绘接线：options computed 依赖隐私开关，切换产出新对象驱动 vue-chartjs 重绘
    const optionsOn = barOptions(wrapper, 0)
    expect(optionsOn).not.toBe(optionsOff)
    expect(linearTick(optionsOn, 'y')(1234560, 0, [])).toBe('••••')
    const label = tooltipCb<(item: TooltipItem<'bar'>) => string>(optionsOn, 'label')
    expect(label(tooltipItem(100000, '收入'))).toBe('收入: ••••')
    const afterBody = tooltipCb<(items: TooltipItem<'bar'>[]) => string>(optionsOn, 'afterBody')
    expect(afterBody(monthlyTooltipItems())).toBe('净额: ••••')
    // x 轴为月份字符串，不含金额（核查记录：月度图仅值轴带金额）
    expect(optionsOn.scales?.x?.ticks?.callback).toBeUndefined()
  })
})

describe('报表卡面：支出分类构成（值轴刻度 / 图内柱尾标注 / tooltip，issue #567）', () => {
  it('值轴刻度与 tooltip 关闭态与现状一致：金额 · 占比%', async () => {
    const wrapper = mount(ReportsView)
    await flushPromises()
    const options = barOptions(wrapper, 1)
    const tick = linearTick(options, 'x')
    expect(tick(6000, 0, [])).toBe('60')
    expect(tick(800, 0, [])).toBe('8')
    const label = tooltipCb<(item: TooltipItem<'bar'>) => string>(options, 'label')
    // 合计 9800：6000 → 61%
    expect(label(tooltipItem(6000))).toBe('60 · 61%')
    // 类目轴（y）为分类名字符串，不带刻度 callback（核查记录：仅值轴带金额）
    expect(options.scales?.y?.ticks?.callback).toBeUndefined()
  })

  it('开启后值轴与 tooltip 恒掩码，占比保留（图形与相对构成可用性不受损）', async () => {
    const wrapper = mount(ReportsView)
    await flushPromises()
    amountPrivacyEnabled.value = true
    await nextTick()
    const options = barOptions(wrapper, 1)
    expect(linearTick(options, 'x')(6000, 0, [])).toBe('••••')
    const label = tooltipCb<(item: TooltipItem<'bar'>) => string>(options, 'label')
    expect(label(tooltipItem(6000))).toBe('•••• · 61%')
    expect(label(tooltipItem(3000))).toBe('•••• · 31%')
  })

  it('图内柱尾金额标注（canvas 插件）开启恒掩码，关闭态与现状一致', async () => {
    const wrapper = mount(ReportsView)
    await flushPromises()
    const plugins = barProp(wrapper, 1, 'plugins') as Array<{
      id: string
      afterDatasetsDraw(chart: Chart<'bar'>): void
    }>
    const endPlugin = plugins.find((p) => p.id === 'barEndAmounts')
    expect(endPlugin).toBeTruthy()

    // 关闭态：柱尾画「60 / 30」（与轴刻度同口径）
    const off = fakeBarEndChart([6000, 3000])
    endPlugin!.afterDatasetsDraw(off.chart)
    expect(off.fillText.mock.calls.map((c) => c[0])).toEqual(['60', '30'])

    amountPrivacyEnabled.value = true
    const on = fakeBarEndChart([6000, 3000])
    endPlugin!.afterDatasetsDraw(on.chart)
    expect(on.fillText.mock.calls.map((c) => c[0])).toEqual(['••••', '••••'])
  })
})

describe('报表卡面：商户排行表格（金额数字同源掩码，issue #567 → #618 表格化）', () => {
  /** 商户表格数据行（NDataTable 渲染的 tbody tr） */
  function merchantRows(wrapper: VueWrapper) {
    return wrapper.findAll('[data-testid="merchant-table"] tbody tr')
  }

  it('关闭态金额与现状逐字符一致（formatAmount 分 → 元）；占比与内嵌条照常渲染', async () => {
    const wrapper = mount(ReportsView)
    await flushPromises()
    const trs = merchantRows(wrapper)
    expect(trs).toHaveLength(2)
    // 50000 → 500、9900 → 99（金额数字走 formatAmount 展示格式化层，掩码单一来源）
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe('500')
    expect(trs[1].find('[data-testid="merchant-amount"]').text()).toBe('99')
    // 占比分母 = 后端全量合计 150000：50000 → 33%、9900 → 7%（误用展示行合计 59900 会得 83%/17%）
    expect(trs[0].find('[data-testid="merchant-share"]').text()).toBe('33%')
    expect(trs[1].find('[data-testid="merchant-share"]').text()).toBe('7%')
    // 内嵌条（形状）照常渲染：条长 ∝ 金额 ÷ 显示区最大金额（50000 为最大 → 100% / 19.8%）
    expect(trs[0].find('[data-testid="merchant-bar"]').attributes('style')).toContain('width: 100%')
    expect(trs[1].find('[data-testid="merchant-bar"]').attributes('style')).toContain('width: 19.8%')
  })

  it('开启后金额数字恒掩码，占比保留（相对构成可用性不受损）、内嵌条（形状）保留', async () => {
    const wrapper = mount(ReportsView)
    await flushPromises()
    amountPrivacyEnabled.value = true
    await nextTick()
    const trs = merchantRows(wrapper)
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe('••••')
    expect(trs[1].find('[data-testid="merchant-amount"]').text()).toBe('••••')
    expect(trs[0].find('[data-testid="merchant-share"]').text()).toBe('33%')
    expect(trs[1].find('[data-testid="merchant-share"]').text()).toBe('7%')
    expect(trs[0].find('[data-testid="merchant-bar"]').exists()).toBe(true)
    expect(trs[0].find('[data-testid="merchant-bar"]').attributes('style')).toContain('width: 100%')
  })
})

describe('投资趋势面：组合 / 单标的（y 轴刻度 / tooltip 同源掩码，issue #567）', () => {
  it('组合模式（金额刻度）开启恒掩码，关闭态与现状逐字符一致', async () => {
    const wrapper = mount(PortfolioTrendPanel)
    await flushPromises()
    const line = wrapper.findComponent({ name: 'Line' })
    expect(line.exists()).toBe(true)
    const options = line.props('options') as ChartOptions<'line'>
    const tick = linearTick(options, 'y')
    expect(tick(100000, 0, [])).toBe('¥1000')
    const label = tooltipCb<(item: TooltipItem<'line'>) => string>(options, 'label')
    expect(label(tooltipItem(100000, '组合市值'))).toBe('组合市值: ¥1000')

    amountPrivacyEnabled.value = true
    await nextTick()
    expect(tick(100000, 0, [])).toBe('••••')
    expect(label(tooltipItem(110000, '组合市值'))).toBe('组合市值: ••••')
    // x 轴为日期字符串，不带金额（核查记录：趋势图仅值轴带金额）
    expect(options.scales?.x?.ticks?.callback).toBeUndefined()
  })

  it('单标的模式（价格刻度 formatPrice）开启恒掩码，关闭态与现状逐字符一致', async () => {
    const wrapper = mount(PortfolioTrendPanel, {
      props: { entryInstrument: stockInstrument },
    })
    await flushPromises()
    const options = wrapper.findComponent({ name: 'Line' }).props('options') as ChartOptions<'line'>
    const tick = linearTick(options, 'y')
    // 1500 万分之一元 → ¥0.15（价格列 ADR-0038 刻度）
    expect(tick(1500, 0, [])).toBe('¥0.15')
    amountPrivacyEnabled.value = true
    await nextTick()
    expect(tick(1500, 0, [])).toBe('••••')
  })
})

describe('订阅花费趋势面（y 轴刻度 / tooltip 同源掩码，issue #567）', () => {
  it('开启恒掩码，关闭态与现状逐字符一致', async () => {
    const wrapper = mount(SubscriptionSpendPanel)
    await flushPromises()
    const options = wrapper.findComponent({ name: 'Bar' }).props('options') as ChartOptions<'bar'>
    const tick = linearTick(options, 'y')
    expect(tick(34800, 0, [])).toBe('¥348')
    const label = tooltipCb<(item: TooltipItem<'bar'>) => string>(options, 'label')
    expect(label(tooltipItem(34800))).toBe('¥348')

    amountPrivacyEnabled.value = true
    await nextTick()
    expect(tick(34800, 0, [])).toBe('••••')
    expect(label(tooltipItem(34800))).toBe('••••')
  })
})
