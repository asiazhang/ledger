import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { setFakeMedia } from './helpers/media-mock'
import ReportsView from '@/views/ReportsView.vue'
import { formatAmount } from '@/utils/money'
import type { ReportDateRange } from '@ledger/types'

/**
 * 报表页触控交互轴（issue #843 / ADR-0088 决策 6，词汇表「输入轴」「分类下钻」）：
 * 分类构成占比指针轴收进悬停 tooltip（行为不变）；触控轴经头部「占比」切换按钮
 * 点按显示——开启后柱尾标注升级为「金额 · 占比%」（口径同 tooltip 单点
 * barTooltipLabel，分母随层级）。图数据形态经图桩断言（jsdom 无 canvas，
 * line-chart-stub 先例）；两轴对比组件测试，换档经媒体查询测试接缝。
 */

const pushMock = vi.fn()
vi.mock('vue-router', () => ({ useRouter: () => ({ push: pushMock }) }))

// jsdom 无 canvas：图表组件用共享桩承接（line-chart-stub，先例 #160/#378）
vi.mock('vue-chartjs', async () => {
  const { BarChartStubWithOptions } = await import('./line-chart-stub')
  return { Bar: BarChartStubWithOptions }
})

const mockRange: ReportDateRange = { min_date: '2020-01-01', max_date: '2027-12-31' }

const mockShares = [
  { category_id: 'food', category_name: '餐饮', amount_cents: 5000 },
  { category_id: 'transport', category_name: '交通', amount_cents: 3000 },
]

beforeEach(() => {
  wireInvokeSeam({
    defaults: {
      report_date_range: mockRange,
      monthly_summary: [],
      category_shares: mockShares,
      merchant_shares: { rows: [], total_cents: 0 },
    },
  })
})

afterEach(() => {
  pushMock.mockReset()
})

async function mountReports() {
  const wrapper = mount(ReportsView)
  await flushPromises()
  return wrapper
}

/** 分类构成图桩的 options（JSON 序列化还原，#378 桩约定） */
function categoryOptions(wrapper: ReturnType<typeof mount>) {
  return JSON.parse(wrapper.find('.category-chart [data-testid="bar-options"]').text())
}

function percentToggle(wrapper: ReturnType<typeof mount>) {
  return wrapper.find('[data-testid="category-percent-toggle"]')
}

describe('报表分类构成占比触控一击可达（issue #843 两轴对比）', () => {
  it('指针轴：无占比切换按钮（hover tooltip 行为不变），柱尾无占比覆盖', async () => {
    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const wrapper = await mountReports()
    expect(percentToggle(wrapper).exists()).toBe(false)
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined()
  })

  it('触控轴：头部「占比」按钮点按显示——柱尾标注升级为「金额 · 占比%」（分母随层级）', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = await mountReports()
    const toggle = percentToggle(wrapper)
    expect(toggle.exists()).toBe(true)
    // 初始：柱尾默认金额，无覆盖
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined()
    expect(toggle.attributes('aria-pressed')).toBe('false')
    // 点按显示：金额 · 占比%（5000/8000 → 63%、3000/8000 → 38%，口径同 tooltip）
    await toggle.trigger('click')
    await flushPromises()
    expect(percentToggle(wrapper).attributes('aria-pressed')).toBe('true')
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toEqual([
      `${formatAmount(5000)} · 63%`,
      `${formatAmount(3000)} · 38%`,
    ])
    // 再点收回：回默认金额标注
    await percentToggle(wrapper).trigger('click')
    await flushPromises()
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined()
  })

  it('占比覆盖不改图数据：柱值与下钻点击载荷不受切换影响', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = await mountReports()
    await percentToggle(wrapper).trigger('click')
    await flushPromises()
    const data = JSON.parse(wrapper.find('.category-chart [data-testid="bar-data"]').text())
    expect(data.datasets[0].data).toEqual([5000, 3000])
    expect(mockInvoke).toHaveBeenCalledWith('category_shares', {
      kind: 'expense',
      month: null,
      year: null,
      from: expect.any(String),
      to: expect.any(String),
    })
  })

  it('换轴即失效：占比开启后切回指针轴，按钮随轴消失且柱尾覆盖不残留', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = await mountReports()
    await percentToggle(wrapper).trigger('click')
    await flushPromises()
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeTruthy()

    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    await flushPromises()
    expect(percentToggle(wrapper).exists()).toBe(false)
    expect(categoryOptions(wrapper).plugins.barEndAmounts.labels).toBeUndefined()
  })
})
