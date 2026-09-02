import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { NSelect } from 'naive-ui'
import ReportsView from '@/views/ReportsView.vue'
import { invokeHandler, makeCategory } from './factories'
import type { ReportDateRange } from '@/types'

// jsdom 无 canvas：图表组件用共享桩承接（line-chart-stub，先例 #160），
// 把 data/options 序列化进 DOM 供断言图数据形态与横向 options。
vi.mock('vue-chartjs', async () => {
  const { BarChartStubWithOptions } = await import('./line-chart-stub')
  return { Bar: BarChartStubWithOptions }
})

const mockInvoke = vi.mocked(invoke)

const currentYear = new Date().getFullYear()

/** 范围夹具：起点比当前年早 6 年、终点在未来年——平铺全集须覆盖滑动窗口之外的年份 */
const mockRange: ReportDateRange = {
  min_date: `${currentYear - 6}-03-01`,
  max_date: `${currentYear + 1}-11-30`,
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

/** 默认 invoke mock：参考数据（reference store self-init）+ 年份范围 + 三报表查询（空集即可） */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: [],
        list_accounts: [],
        list_categories: [],
        list_merchants: [],
        report_date_range: mockRange,
        monthly_summary: [],
        category_shares: [],
        merchant_shares: [],
      },
      extra,
    ),
  )
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
})

async function mountReports() {
  const wrapper = mount(ReportsView)
  await flushPromises()
  return wrapper
}

function yearOptionsOf(wrapper: ReturnType<typeof mount>) {
  return wrapper.findComponent(NSelect).props('options') as {
    label: string
    value: number
  }[]
}

/** 支出分类构成图（class 定位，不依赖月度图是否渲染）的 data/options */
function categoryChartProp(prop: 'data' | 'options', wrapper: ReturnType<typeof mount>) {
  const node = wrapper.find(`.category-chart [data-testid="bar-${prop}"]`)
  return JSON.parse(node.text())
}

describe('ReportsView 年份筛选（issue #267）', () => {
  it('挂载时拉取范围一次，选项为范围内全部年份升序平铺、纯数字 label', async () => {
    const wrapper = await mountReports()
    const expected = Array.from({ length: 8 }, (_, i) => currentYear - 6 + i)
    expect(yearOptionsOf(wrapper).map((o) => o.value)).toEqual(expected)
    expect(yearOptionsOf(wrapper).map((o) => o.label)).toEqual(expected.map(String))
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_date_range')
    expect(rangeCalls).toHaveLength(1)
  })

  it('空库日期范围回退单当前年', async () => {
    baseInvoke({ report_date_range: { min_date: null, max_date: null } })
    const wrapper = await mountReports()
    expect(yearOptionsOf(wrapper).map((o) => o.value)).toEqual([currentYear])
  })

  it('±2 滑动窗口已删除：远早于当前年的年份一击直达', async () => {
    const wrapper = await mountReports()
    const values = yearOptionsOf(wrapper).map((o) => o.value)
    expect(values).toContain(currentYear - 6)
    expect(values).toContain(currentYear + 1)
    // 选项全集恰为范围内年份，不多不少
    expect(values).toHaveLength(8)
  })

  it('默认选中当前年（范围内天然包含，无需钳制）', async () => {
    const wrapper = await mountReports()
    expect(wrapper.findComponent(NSelect).props('value')).toBe(currentYear)
  })

  it('分类份额随年份联动（issue #376）：初始加载携带当前年', async () => {
    await mountReports()
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', {
      kind: 'expense',
      month: null,
      year: currentYear,
    })
  })

  it('切换年份触发三个报表查询且年份参数正确（联动刷新），范围不重复拉取', async () => {
    const wrapper = await mountReports()
    mockInvoke.mockClear()
    wrapper.findComponent(NSelect).vm.$emit('update:value', currentYear - 6)
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('monthly_summary', { year: currentYear - 6 })
    expect(mockInvoke).toHaveBeenCalledWith('merchant_shares', { year: currentYear - 6 })
    // 分类份额随年份联动（issue #376）：口径修复，不再全时段
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', {
      kind: 'expense',
      month: null,
      year: currentYear - 6,
    })
    const rangeCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'report_date_range')
    expect(rangeCalls).toHaveLength(0)
  })
})

describe('ReportsView 支出分类构成横向柱状图（issue #378）', () => {
  const mountWithShares = mountReports

  it('横向柱状图：indexAxis 为 y，图卡正常挂载', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountWithShares()
    const options = categoryChartProp('options', wrapper)
    expect(options.indexAxis).toBe('y')
    expect(options.plugins.legend.display).toBe(false)
    expect(wrapper.find('.category-chart [data-testid="bar-data"]').exists()).toBe(true)
  })

  it('图数据形态：一级归并（二级并入根）+ 未分类柱，净额降序，净额 0 不进图', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountWithShares()
    const data = categoryChartProp('data', wrapper)
    // 餐饮 = 根 5000 + 二级零食 1000；降序：餐饮 6000 > 交通 3000 > 未分类 800
    expect(data.labels).toEqual(['餐饮', '交通', '未分类'])
    expect(data.datasets[0].data).toEqual([6000, 3000, 800])
  })

  it('配色：与柱同序按 id 稳定取色，未分类固定灰', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountWithShares()
    const data = categoryChartProp('data', wrapper)
    const colors: string[] = data.datasets[0].backgroundColor
    expect(colors).toHaveLength(3)
    // 未分类（第三根柱）固定灰
    expect(colors[2]).toBe('#909399')
    // 两个真实分类各自稳定取色且不同柱序不改变颜色
    expect(colors[0]).not.toBe('#909399')
    expect(colors[1]).not.toBe(colors[0])
  })

  it('同分类跨年份颜色稳定：切年后同 id 的柱颜色不变', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountWithShares()
    const before = categoryChartProp('data', wrapper).datasets[0].backgroundColor
    wrapper.findComponent(NSelect).vm.$emit('update:value', currentYear - 6)
    await flushPromises()
    const after = categoryChartProp('data', wrapper).datasets[0].backgroundColor
    expect(after).toEqual(before)
  })

  it('全部平铺、卡片内滚动：容器限高滚动、图高随行数增长不截断', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountWithShares()
    const scroll = wrapper.find('[data-testid="category-chart-scroll"]')
    expect(scroll.exists()).toBe(true)
    expect(scroll.attributes('style')).toContain('overflow-y: auto')
    // 3 根柱 × 行高 32px，图高随行数平铺而非固定视口
    const inner = wrapper.find('[data-testid="category-chart-canvas"]')
    expect(inner.attributes('style')).toContain('height: 192px')
  })

  it('汇总层级切换器已退役：不再渲染 NRadioGroup', async () => {
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountWithShares()
    expect(wrapper.findComponent({ name: 'NRadioGroup' }).exists()).toBe(false)
  })

  it('localStorage 残留汇总层级键无副作用：原样保留、渲染正常、不写回', async () => {
    localStorage.setItem('view_state:reports_group_level', '"level1"')
    baseInvoke({ list_categories: mockCategories, category_shares: mockShares })
    const wrapper = await mountWithShares()
    // 残留键不读、不写、不清：视图不感知该键
    expect(localStorage.getItem('view_state:reports_group_level')).toBe('"level1"')
    // 渲染不受残留键影响：图数据与配色照常
    expect(categoryChartProp('data', wrapper).datasets[0].data).toEqual([6000, 3000, 800])
    localStorage.removeItem('view_state:reports_group_level')
  })
})
