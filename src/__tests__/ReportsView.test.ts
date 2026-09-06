import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { NButton, NEmpty, NSelect } from 'naive-ui'
import QuickTimeRange from '@/components/QuickTimeRange.vue'
import ReportsView from '@/views/ReportsView.vue'
import { categoryColor } from '@/utils/category-chart'
import { UNCATEGORIZED_ONLY, CATEGORY_DRILLDOWN_KINDS, MERCHANT_DRILLDOWN_KINDS } from '@/composables/useTransactionFilter'
import { invokeHandler, makeCategory } from './factories'
import type { NullableDateRange } from '@/utils/time-period'
import type { ReportDateRange } from '@/types'

// jsdom 无 canvas：图表组件用共享桩承接（line-chart-stub，先例 #160），
// 把 data/options 序列化进 DOM 供断言图数据形态与横向 options。
vi.mock('vue-chartjs', async () => {
  const { BarChartStubWithOptions } = await import('./line-chart-stub')
  return { Bar: BarChartStubWithOptions }
})

// 跳转下钻（issue #380）：视图经 router.push 跳交易列表，mock 后断言跳转载荷
const pushMock = vi.fn()
vi.mock('vue-router', () => ({ useRouter: () => ({ push: pushMock }) }))


// 固定「今天」= 2026-01-15（本地）：默认「当年」快照与芯片换算随之确定
// （TransactionsView 时间维度行测试同款前提），期望年份一律用字面量 2026。
const Y = 2026

/** 范围夹具：QuickTimeRange 钳制输入（视图不再自拉范围），界内界外年份皆覆盖 */
const mockRange: ReportDateRange = {
  min_date: '2020-03-01',
  max_date: '2027-11-30',
}

/** 分类夹具：food 下挂二级 snack（归并断言依赖父子关系） */
const mockCategories = [
  makeCategory({ id: 'food', name: '餐饮', sort_order: 0 }),
  makeCategory({ id: 'food-snack', name: '零食', parent_id: 'food', sort_order: 1 }),
  makeCategory({ id: 'transport', name: '交通', sort_order: 2 }),
]

/** 分类份额夹具：后端叶子级行（ORDER BY net DESC），未分类 category_id 为空串 */
const mockShares = [
  { category_id: 'food', category_name: '餐饮', amount_cents: 5000 },
  { category_id: 'transport', category_name: '交通', amount_cents: 3000 },
  { category_id: 'food-snack', category_name: '零食', amount_cents: 1000 },
  { category_id: '', category_name: '未分类', amount_cents: 800 },
  { category_id: 'zero', category_name: '零净额', amount_cents: 0 },
]

/** 默认 invoke mock：参考数据（reference store self-init）+ 期间边界（组件钳制输入）
 * + 三报表查询（空集即可） */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: [],
        list_accounts: [],
        list_categories: [],
        list_merchants: [],
        list_insurers: [],
        report_date_range: mockRange,
        monthly_summary: [],
        category_shares: [],
        merchant_shares: { rows: [], total_cents: 0 },
      },
      extra,
    ),
  )
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  pushMock.mockReset()
  vi.useFakeTimers()
  vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0))
})

afterEach(() => {
  vi.useRealTimers()
})

async function mountReports() {
  const wrapper = mount(ReportsView)
  await flushPromises()
  return wrapper
}

/** 芯片按钮按文案定位（报表闭集文案唯一：当月/当季/当年/去年） */
const chip = (wrapper: ReturnType<typeof mount>, label: string) =>
  wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!

async function clickChip(wrapper: ReturnType<typeof mount>, label: string) {
  await chip(wrapper, label).trigger('click')
  await flushPromises()
}

/** 点第 i 根分类柱（经图桩按钮按 chart.js onClick 契约回调；#379 与 #427 两块共用） */
async function clickBar(wrapper: ReturnType<typeof mount>, index: number) {
  await wrapper.findAll('[data-testid="bar-click"]')[index].trigger('click')
  await flushPromises()
}

/** 图内下钻面包屑（存在 = 下钻态；#379 与 #427 两块共用） */
function breadcrumbOf(wrapper: ReturnType<typeof mount>) {
  return wrapper.find('[data-testid="category-breadcrumb"]')
}

/** 经共享受控组件 emit 期间快照（v-model 回流视图），模拟步进/面板产出的任意期间 */
async function emitPeriod(wrapper: ReturnType<typeof mount>, range: NullableDateRange) {
  wrapper.findComponent(QuickTimeRange).vm.$emit('update:modelValue', range)
  await flushPromises()
}

/** 月度收支卡（DOM 中第一张 Bar 图桩）的 data/options */
function monthlyChartProp(prop: 'data' | 'options', wrapper: ReturnType<typeof mount>) {
  const node = wrapper.findAll(`[data-testid="bar-${prop}"]`)[0]
  return JSON.parse(node.text())
}

/** 支出分类构成图（class 定位）的 data/options */
function categoryChartProp(prop: 'data' | 'options', wrapper: ReturnType<typeof mount>) {
  const node = wrapper.find(`.category-chart [data-testid="bar-${prop}"]`)
  return JSON.parse(node.text())
}

describe('ReportsView 期间筛选（issue #411 / ADR-0057）', () => {
  it('进入默认「当年」快照：三卡以当年期间查询，期间边界由组件内化拉取一次', async () => {
    await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    })
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    })
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', {
      kind: 'expense',
      month: null,
      year: null,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    })
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_date_range')
    expect(rangeCalls).toHaveLength(1)
  })

  it('年份下拉退役：无年份选择下拉，快捷选择行为唯一时间控件（四枚芯片、无「全部」）', async () => {
    const wrapper = await mountReports()
    expect(wrapper.findComponent(NSelect).exists()).toBe(false)
    expect(wrapper.findComponent(QuickTimeRange).exists()).toBe(true)
    // 报表页日期闭集：仅当月/当季/当年/去年，无「全部」（期间必有界）
    for (const label of ['当月', '当季', '当年', '去年']) {
      expect(chip(wrapper, label).exists()).toBe(true)
    }
    expect(chip(wrapper, '全部')).toBeUndefined()
  })

  it('点「去年」芯片：三卡以去年期间重算，边界不重复拉取', async () => {
    const wrapper = await mountReports()
    mockInvoke.mockClear()
    await clickChip(wrapper, '去年')
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    })
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
      topN: 5,
    })
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', {
      kind: 'expense',
      month: null,
      year: null,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    })
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_date_range')
    expect(rangeCalls).toHaveLength(0)
  })

  it('重复点同一段期间的芯片不重复刷新（同值守卫）', async () => {
    const wrapper = await mountReports()
    mockInvoke.mockClear()
    await clickChip(wrapper, '当年')
    const reportCalls = mockInvoke.mock.calls.filter(([cmd]) =>
      ['monthly_summary', 'category_shares', 'merchant_shares'].includes(cmd),
    )
    expect(reportCalls).toHaveLength(0)
  })

  it('点「当月」芯片：月度收支卡单组柱如实展示（月期间不切日粒度）', async () => {
    baseInvoke({
      monthly_summary: [{ month: `${Y}-01`, income_cents: 1000, expense_cents: 500, refund_cents: 100 }],
    })
    const wrapper = await mountReports()
    mockInvoke.mockClear()
    await clickChip(wrapper, '当月')
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-01-31`,
    })
    const data = monthlyChartProp('data', wrapper)
    expect(data.labels).toEqual([`${Y}-01`])
    expect(data.datasets.map((d: { data: number[] }) => d.data)).toEqual([[1000], [500], [100]])
  })

  it('所选期间无流水：三卡显示空态而非旧数据残留', async () => {
    const wrapper = await mountReports()
    const empties = wrapper.findAllComponents(NEmpty)
    expect(empties.length).toBeGreaterThanOrEqual(3)
    expect(wrapper.text()).toContain('本期暂无数据')
    expect(wrapper.text()).toContain('暂无支出数据')
    expect(wrapper.text()).toContain('本期暂无商户消费')
  })

  it('期间选择不持久化：切换期间 localStorage 零写入', async () => {
    const wrapper = await mountReports()
    const keysBefore = Object.keys(localStorage)
    await clickChip(wrapper, '去年')
    expect(Object.keys(localStorage)).toEqual(keysBefore)
  })
})

describe('ReportsView 支出分类构成横向柱状图（issue #378）', () => {
  it('横向柱状图：indexAxis 为 y，图卡正常挂载', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    const options = categoryChartProp('options', wrapper)
    expect(options.indexAxis).toBe('y')
    expect(options.plugins.legend.display).toBe(false)
    expect(wrapper.find('.category-chart [data-testid="bar-data"]').exists()).toBe(true)
  })

  it('图数据形态：一级归并（二级并入根）+ 未分类柱，净额降序，净额 0 不进图', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    const data = categoryChartProp('data', wrapper)
    // 餐饮 = 根 5000 + 二级零食 1000；降序：餐饮 6000 > 交通 3000 > 未分类 800
    expect(data.labels).toEqual(['餐饮', '交通', '未分类'])
    expect(data.datasets[0].data).toEqual([6000, 3000, 800])
  })

  it('配色：与柱同序按 id 稳定取色，未分类固定灰', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    const data = categoryChartProp('data', wrapper)
    const colors: string[] = data.datasets[0].backgroundColor
    expect(colors).toHaveLength(3)
    // 未分类（第三根柱）固定灰
    expect(colors[2]).toBe('#909399')
    // 两个真实分类各自稳定取色且不同柱序不改变颜色
    expect(colors[0]).not.toBe('#909399')
    expect(colors[1]).not.toBe(colors[0])
  })

  it('同分类跨期间颜色稳定：切期间后同 id 的柱颜色不变', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    const before = categoryChartProp('data', wrapper).datasets[0].backgroundColor
    await clickChip(wrapper, '去年')
    const after = categoryChartProp('data', wrapper).datasets[0].backgroundColor
    expect(after).toEqual(before)
  })

  it('全部平铺、卡片内滚动：容器限高滚动、图高随行数增长不截断', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    const scroll = wrapper.find('[data-testid="category-chart-scroll"]')
    expect(scroll.exists()).toBe(true)
    expect(scroll.attributes('style')).toContain('overflow-y: auto')
    // 3 根柱 × 行高 32px，图高随行数平铺而非固定视口
    const inner = wrapper.find('[data-testid="category-chart-canvas"]')
    expect(inner.attributes('style')).toContain('height: 192px')
  })

  it('汇总层级切换器已退役：不再渲染 NRadioGroup', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    expect(wrapper.findComponent({ name: 'NRadioGroup' }).exists()).toBe(false)
  })

  it('localStorage 残留汇总层级键无副作用：原样保留、渲染正常、不写回', async () => {
    localStorage.setItem('view_state:reports_group_level', '"level1"')
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    // 残留键不读、不写、不清：视图不感知该键
    expect(localStorage.getItem('view_state:reports_group_level')).toBe('"level1"')
    // 渲染不受残留键影响：图数据与配色照常
    expect(categoryChartProp('data', wrapper).datasets[0].data).toEqual([6000, 3000, 800])
    localStorage.removeItem('view_state:reports_group_level')
  })
})

describe('ReportsView 分类图内下钻 + 面包屑（issue #379）', () => {
  it('点一级柱图内下钻：行集合 = 直挂行 + 二级子分类行，合计 = 父柱金额（不触发跳转）', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    await clickBar(wrapper, 0) // 餐饮柱（6000 = 直挂 5000 + 零食 1000）
    const data = categoryChartProp('data', wrapper)
    expect(data.labels).toEqual(['餐饮（直挂）', '零食'])
    expect(data.datasets[0].data).toEqual([5000, 1000])
    expect(data.datasets[0].data.reduce((a: number, b: number) => a + b, 0)).toBe(6000)
    // 一级柱是图内下钻不是跳转下钻（issue #380 两段式的第一段）
    expect(pushMock).not.toHaveBeenCalled()
  })

  it('下钻态配色沿用同一稳定映射：直挂行同父柱色，二级行同分类色', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    const baseColors: string[] = categoryChartProp('data', wrapper).datasets[0].backgroundColor
    await clickBar(wrapper, 0)
    const colors: string[] = categoryChartProp('data', wrapper).datasets[0].backgroundColor
    // 直挂行（第一根）= 基础态餐饮柱色；零级行 = 同 id 稳定取色
    expect(colors[0]).toBe(baseColors[0])
    expect(colors[1]).toBe(categoryColor('food-snack'))
  })

  it('面包屑显示当前位置（全部分类 › 分类名），点根返回基础态', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    expect(breadcrumbOf(wrapper).exists()).toBe(false)
    await clickBar(wrapper, 0)
    expect(breadcrumbOf(wrapper).exists()).toBe(true)
    expect(breadcrumbOf(wrapper).text()).toContain('全部分类')
    expect(breadcrumbOf(wrapper).text()).toContain('餐饮')
    await wrapper.find('[data-testid="breadcrumb-root"]').trigger('click')
    await flushPromises()
    expect(breadcrumbOf(wrapper).exists()).toBe(false)
    expect(categoryChartProp('data', wrapper).labels).toEqual(['餐饮', '交通', '未分类'])
  })

  it('未分类柱不进图内下钻，直达「仅无分类」列表：载荷 = 保留值 + 当年首尾日期 + 收支类型集合（issue #581）', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    await clickBar(wrapper, 2) // 未分类柱
    expect(pushMock).toHaveBeenCalledTimes(1)
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        category: UNCATEGORIZED_ONLY,
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    })
    // 未分类是柱不是层级：图仍在基础态（面包屑不出现）
    expect(breadcrumbOf(wrapper).exists()).toBe(false)
    expect(categoryChartProp('data', wrapper).labels).toEqual(['餐饮', '交通', '未分类'])
  })

  it('下钻态点二级子分类行：跳转该分类精确过滤，载荷带当年首尾日期', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    await clickBar(wrapper, 0) // 下钻餐饮
    pushMock.mockClear()
    await clickBar(wrapper, 1) // 零食（二级）行
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        category: 'food-snack',
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    })
  })

  it('下钻态点父直挂行：按父分类精确过滤（载荷 category = 父分类 id）', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    await clickBar(wrapper, 0) // 下钻餐饮
    pushMock.mockClear()
    await clickBar(wrapper, 0) // 餐饮（直挂）行
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        category: 'food',
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    })
  })

  it('跳转载荷 = 所选期间首尾日期（#412 期间化）：「去年」芯片后未分类柱带去年年界', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    await clickChip(wrapper, '去年')
    await clickBar(wrapper, 2) // 未分类柱
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        category: UNCATEGORIZED_ONLY,
        dateFrom: `${Y - 1}-01-01`,
        dateTo: `${Y - 1}-12-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    })
  })

  it('跳转载荷随月期间（#412）：「当月」芯片后未分类柱带当月月界', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    await clickChip(wrapper, '当月')
    await clickBar(wrapper, 2) // 未分类柱
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        category: UNCATEGORIZED_ONLY,
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-01-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    })
  })

  it('跳转载荷随季期间（#412）：「当季」芯片后下钻二级行带当季季界', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    await clickChip(wrapper, '当季')
    await clickBar(wrapper, 0) // 图内下钻餐饮
    pushMock.mockClear()
    await clickBar(wrapper, 1) // 零食（二级）行
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        category: 'food-snack',
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-03-31`,
        kinds: CATEGORY_DRILLDOWN_KINDS,
      },
    })
  })

  it('切换期间复位基础态；下钻态不持久化（localStorage 零写入）', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    const keysBefore = Object.keys(localStorage)
    await clickBar(wrapper, 0)
    expect(Object.keys(localStorage)).toEqual(keysBefore)
    await clickChip(wrapper, '去年')
    expect(breadcrumbOf(wrapper).exists()).toBe(false)
    expect(categoryChartProp('data', wrapper).labels).toEqual(['餐饮', '交通', '未分类'])
  })

  it('步进器/面板产出的任意月期间同样驱动三卡重算（受控 v-model 桥接）', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountReports()
    mockInvoke.mockClear()
    // 任意历史月（面板可达）：视图按快照区间查询
    await emitPeriod(wrapper, { from: '2025-12-01', to: '2025-12-31' })
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: 2025,
      from: '2025-12-01',
      to: '2025-12-31',
    })
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', {
      kind: 'expense',
      month: null,
      year: null,
      from: '2025-12-01',
      to: '2025-12-31',
    })
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: 2025,
      from: '2025-12-01',
      to: '2025-12-31',
      topN: 5,
    })
  })
})

describe('ReportsView 会话内保留（issue #427）：同一 pinia 卸载重挂恢复，新 pinia 冷启动', () => {
  it('选期间 + 图内下钻后卸载重挂（同一会话）：期间与下钻面包屑恢复，三卡以恢复期间重拉非缓存数据', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const first = await mountReports()
    await clickChip(first, '去年')
    await clickBar(first, 0) // 图内下钻餐饮
    expect(breadcrumbOf(first).exists()).toBe(true)
    first.unmount()

    // 离开期间新记的账：重挂后三卡以恢复期间重新拉取，渲染新返回值（餐饮 9000 ≠ 离开前 6000）
    const freshShares = [{ category_id: 'food', category_name: '餐饮', amount_cents: 9000 }]
    baseInvoke({ list_categories: mockCategories, category_shares: freshShares })
    mockInvoke.mockClear()
    const second = await mountReports()

    // 三张卡按恢复的「去年」期间重新查询（非缓存数据）
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    })
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', {
      kind: 'expense',
      month: null,
      year: null,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    })
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
      topN: 5,
    })

    // 图内下钻位置恢复：面包屑仍在（全部分类 › 餐饮），图为下钻态行集合
    expect(breadcrumbOf(second).exists()).toBe(true)
    expect(breadcrumbOf(second).text()).toContain('餐饮')
    const data = categoryChartProp('data', second)
    expect(data.labels).toEqual(['餐饮（直挂）'])
    expect(data.datasets[0].data).toEqual([9000])

    // 路由 URL 不变：重挂本身不产生任何跳转
    expect(pushMock).not.toHaveBeenCalled()
  })

  it('恢复期间点亮对应芯片：去年快照恢复后「去年」芯片高亮（primary）', async () => {
    baseInvoke()
    const first = await mountReports()
    await clickChip(first, '去年')
    first.unmount()

    const second = await mountReports()
    const lastYearChip = chip(second, '去年')
    expect(lastYearChip.props('type')).toBe('primary')
    expect(chip(second, '当年').props('type')).toBe('default')
  })

  it('多次往返（报表 → 交易 → 报表 → 更多 → 报表）：恢复最近一次离开时的样子', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    // 第一次进入：切「当月」后离开
    const first = await mountReports()
    await clickChip(first, '当月')
    first.unmount()
    // 第二次进入（第一次「回来」）：恢复当月，再切「去年」+ 下钻后离开
    const second = await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-01-31`,
    })
    await clickChip(second, '去年')
    await clickBar(second, 0) // 图内下钻餐饮
    second.unmount()

    // 第三次进入：恢复最近一次离开（去年 + 餐饮下钻），而非更早的当月
    mockInvoke.mockClear()
    const third = await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: Y - 1,
      from: `${Y - 1}-01-01`,
      to: `${Y - 1}-12-31`,
    })
    expect(breadcrumbOf(third).exists()).toBe(true)
    expect(breadcrumbOf(third).text()).toContain('餐饮')
  })

  it('新 pinia + 重挂表达冷启动：回默认「当年」，下钻回基础态，面包屑不出现', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const first = await mountReports()
    await clickChip(first, '去年')
    await clickBar(first, 0)
    first.unmount()

    // 新 pinia = 新会话（应用重启）：默认当年、无面包屑、图为基础态
    setActivePinia(createPinia())
    mockInvoke.mockClear()
    const second = await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
    })
    expect(breadcrumbOf(second).exists()).toBe(false)
    expect(categoryChartProp('data', second).labels).toEqual(['餐饮', '交通', '未分类'])
  })

  it('会话内保留零持久化：选择期间与下钻全程 localStorage 零写入', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const first = await mountReports()
    const keysBefore = Object.keys(localStorage)
    await clickChip(first, '去年')
    await clickBar(first, 0)
    first.unmount()
    const second = await mountReports()
    expect(Object.keys(localStorage)).toEqual(keysBefore)
    second.unmount()
  })
})


describe('ReportsView 商户排行表格化 + TopN（issue #588 → #618）', () => {
  const mockMerchants = [
    { merchant_id: 'm-1', merchant_name: '超市', amount_cents: 5000, transaction_count: 3 },
    { merchant_id: 'm-2', merchant_name: '咖啡', amount_cents: 3000, transaction_count: 2 },
    { merchant_id: 'm-3', merchant_name: '书店', amount_cents: 1000, transaction_count: 1 },
  ]

  /** 商户载荷：total_cents 刻意 ≠ rows 合计（9000），供占比分母断言识别真源 */
  const merchantPayload = (rows = mockMerchants) => ({ rows, total_cents: 15000 })

  /** 商户参考数据：MerchantLink 经 merchantMap 解析名称（软删历史名照常可下钻） */
  const merchantRefs = mockMerchants.map((m) => ({
    id: m.merchant_id,
    name: m.merchant_name,
    is_deleted: false,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
  }))

  /** 点第 i 行商户名（MerchantLink 受控下钻模式，#618 表格化后的下钻入口） */
  async function clickMerchantName(wrapper: ReturnType<typeof mount>, index = 0) {
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr')
    await trs[index].find('[data-testid="merchant-name"]').trigger('click')
    await flushPromises()
  }

  /** TopN 档位选择（面板头部 NRadioButton 按档位定位；naive-ui 交互走 input setValue，
   * PortfolioTrendPanel.test.ts 同款） */
  async function clickTopN(wrapper: ReturnType<typeof mount>, n: number) {
    await wrapper
      .find(`[data-testid="merchant-topn-${n}"] input`)
      .setValue(true)
    await flushPromises()
  }

  it('表格行序 = 后端返回序：商户名、金额、占比、笔数逐行渲染（口径归纯函数，此处锁视图接线）', async () => {
    baseInvoke({ list_merchants: merchantRefs, merchant_shares: merchantPayload() })
    const wrapper = await mountReports()
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr')
    expect(trs).toHaveLength(3)
    expect(trs[0].find('[data-testid="merchant-name"]').text()).toBe('超市')
    // 金额走 formatAmount（分 → 元）：5000 → 50；占比分母 = 载荷全量合计 15000 → 33%
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe('50')
    expect(trs[0].find('[data-testid="merchant-share"]').text()).toBe('33%')
    expect(trs[0].find('[data-testid="merchant-count"]').text()).toBe('3')
  })

  it('默认 Top 5：进入即以 top_n=5 查询', async () => {
    baseInvoke({ merchant_shares: merchantPayload() })
    await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    })
  })

  it('切 Top 10：仅商户卡以 top_n=10 重拉，其余两卡不受牵连', async () => {
    baseInvoke({ merchant_shares: merchantPayload() })
    const wrapper = await mountReports()
    mockInvoke.mockClear()
    await clickTopN(wrapper, 10)
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 10,
    })
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'monthly_summary')).toHaveLength(0)
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === 'category_shares')).toHaveLength(0)
  })

  it('TopN 会话内保留：同 pinia 卸载重挂以 Top 10 重拉；冷启动（新 pinia）回默认 5', async () => {
    baseInvoke({ merchant_shares: merchantPayload() })
    const first = await mountReports()
    await clickTopN(first, 10)
    first.unmount()

    mockInvoke.mockClear()
    const second = await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 10,
    })
    second.unmount()

    setActivePinia(createPinia())
    mockInvoke.mockClear()
    await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', {
      year: Y,
      from: `${Y}-01-01`,
      to: `${Y}-12-31`,
      topN: 5,
    })
  })

  it('TopN 切换零持久化：localStorage 零写入', async () => {
    baseInvoke({ merchant_shares: merchantPayload() })
    const wrapper = await mountReports()
    const keysBefore = Object.keys(localStorage)
    await clickTopN(wrapper, 10)
    expect(Object.keys(localStorage)).toEqual(keysBefore)
  })

  it('TopN 快速连点竞态：最后一次发起胜出，迟到的前发响应丢弃（ADR-0040 同语义）', async () => {
    baseInvoke({ merchant_shares: merchantPayload() })
    const wrapper = await mountReports()
    // 第 2 次发起（top_n=10）挂起；第 3 次发起（top_n=5）立即返回新数据
    let releaseTop10!: (v: unknown) => void
    const pendingTop10 = new Promise((resolve) => {
      releaseTop10 = resolve
    })
    const top5Payload = {
      rows: [{ merchant_id: 'm-9', merchant_name: '快餐', amount_cents: 500, transaction_count: 2 }],
      total_cents: 15000,
    }
    mockInvoke.mockImplementation((cmd: string, args: Record<string, unknown>) => {
      if (cmd === 'merchant_shares') {
        if (args.topN === 10) return pendingTop10
        return Promise.resolve(top5Payload)
      }
      return Promise.resolve([])
    })
    await clickTopN(wrapper, 10) // 发起 #2：挂起
    await clickTopN(wrapper, 5) // 发起 #3：立即落位
    // 迟到的 #2（top 10 旧响应）后到：必须被丢弃，不得覆盖 top 5 结果
    releaseTop10({ rows: [{ merchant_id: 'm-x', merchant_name: '迟到户', amount_cents: 9, transaction_count: 1 }], total_cents: 9 })
    await flushPromises()
    const trs = wrapper.findAll('[data-testid="merchant-table"] tbody tr')
    expect(trs).toHaveLength(1)
    expect(trs[0].find('[data-testid="merchant-amount"]').text()).toBe('5')
  })

  it('点商户名跳传交易列表（#589 → #618）：载荷 = 商户 id + 所选期间首尾日期 + 收支类型集合（支出+退款）', async () => {
    baseInvoke({ list_merchants: merchantRefs, merchant_shares: merchantPayload() })
    const wrapper = await mountReports()
    pushMock.mockClear()
    // 点第 1 行商户名（超市 5000，商户 id m-1）：直达该商户本期支出+退款明细
    await clickMerchantName(wrapper)
    expect(pushMock).toHaveBeenCalledTimes(1)
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        merchant: 'm-1',
        dateFrom: `${Y}-01-01`,
        dateTo: `${Y}-12-31`,
        kinds: MERCHANT_DRILLDOWN_KINDS,
      },
    })
  })

  it('商户下钻载荷随期间（#589 边界）：选「去年」后点商户名带去年年界', async () => {
    baseInvoke({ list_merchants: merchantRefs, merchant_shares: merchantPayload() })
    const wrapper = await mountReports()
    await clickChip(wrapper, '去年')
    pushMock.mockClear()
    await clickMerchantName(wrapper)
    expect(pushMock).toHaveBeenCalledWith({
      name: 'transactions',
      query: {
        merchant: 'm-1',
        dateFrom: `${Y - 1}-01-01`,
        dateTo: `${Y - 1}-12-31`,
        kinds: MERCHANT_DRILLDOWN_KINDS,
      },
    })
  })
})
