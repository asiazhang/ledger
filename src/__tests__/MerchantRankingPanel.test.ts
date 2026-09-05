import { describe, it, expect, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia } from 'pinia'
import MerchantRankingPanel from '@/components/reports/MerchantRankingPanel.vue'
import { paletteColor } from '@/utils/category-chart'
import type { TooltipItem } from 'chart.js'
import type { MerchantSharesReport } from '@/types'

// jsdom 无 canvas：图桩承接（line-chart-stub 先例），把 data/options 序列化进 DOM
// 供断言图数据形态；tooltip 回调经 props 真对象直接调用。
vi.mock('vue-chartjs', async () => {
  const { BarChartStubWithOptions } = await import('./line-chart-stub')
  return { Bar: BarChartStubWithOptions }
})

/** 夹具即后端返回顺序（净额降序 + topN 截断已收口后端），面板只渲染不再排序。 */
const rows = [
  { merchant_id: 'm1', merchant_name: '京东', amount_cents: 170000, transaction_count: 9 },
  { merchant_id: 'm2', merchant_name: '红旗连锁', amount_cents: 100000, transaction_count: 5 },
]
// total_cents 刻意 ≠ rows 合计（270000）：分母断言可辨真源
const report: MerchantSharesReport = { rows, total_cents: 340000 }

function mountPanel(props: { report?: MerchantSharesReport; topN?: number } = {}) {
  return mount(MerchantRankingPanel, {
    props: { report: props.report ?? report, topN: props.topN ?? 5 },
    global: { plugins: [createPinia()] },
  })
}

function chartDataOf(wrapper: ReturnType<typeof mount>) {
  return JSON.parse(wrapper.find('.merchant-chart [data-testid="bar-data"]').text())
}

function chartOptionsOf(wrapper: ReturnType<typeof mount>) {
  return wrapper.getComponent('.merchant-chart').props('options')
}

describe('MerchantRankingPanel（issue #588 柱图化）', () => {
  it('横向柱状图：柱序 = 后端返回序，名称进轴标签、金额进柱数据', () => {
    const wrapper = mountPanel()
    const data = chartDataOf(wrapper)
    expect(data.labels).toEqual(['京东', '红旗连锁'])
    expect(data.datasets[0].data).toEqual([170000, 100000])
  })

  it('柱色与分类构成同源：色板按名次序取色（多颜色）', () => {
    const wrapper = mountPanel()
    const colors: string[] = chartDataOf(wrapper).datasets[0].backgroundColor
    expect(colors).toEqual([paletteColor(0), paletteColor(1)])
  })

  it('tooltip「金额 · 占比%」分母 = 载荷全量合计，非展示行合计', () => {
    const wrapper = mountPanel()
    const options = chartOptionsOf(wrapper)
    const label = options.plugins?.tooltip?.callbacks
      ?.label as (item: TooltipItem<'bar'>) => string
    // 京东 170000 / 全量 340000 = 50%（误用展示行合计 270000 会得 63%）
    expect(label({ raw: 170000 } as TooltipItem<'bar'>)).toBe('1700 · 50%')
    expect(label({ raw: 100000 } as TooltipItem<'bar'>)).toBe('1000 · 29%')
  })

  it('TopN 档位受控渲染与切换上报：emit update:topN', async () => {
    const wrapper = mountPanel({ topN: 10 })
    // 当前档位 Top 10：第二枚选中（naive-ui 受控）
    expect(
      (wrapper.find('[data-testid="merchant-topn-10"] input').element as HTMLInputElement)
        .checked,
    ).toBe(true)
    // 切到 Top 5：受控上报，组件不持状态源
    await wrapper.find('[data-testid="merchant-topn-5"] input').setValue(true)
    expect(wrapper.emitted('update:topN')).toEqual([[5]])
  })

  it('空排行显示空态提示（NEmpty 保留），不渲染图', () => {
    const wrapper = mountPanel({ report: { rows: [], total_cents: 0 } })
    expect(wrapper.find('[data-testid="merchant-empty"]').exists()).toBe(true)
    expect(wrapper.find('.merchant-chart').exists()).toBe(false)
  })

  it('点商户柱上报下钻意图：emit drilldown（携带 merchant_id，issue #589）', async () => {
    const wrapper = mountPanel()
    await wrapper.findAll('[data-testid="bar-click"]')[0].trigger('click')
    expect(wrapper.emitted('drilldown')).toEqual([['m1']])
    await wrapper.findAll('[data-testid="bar-click"]')[1].trigger('click')
    expect(wrapper.emitted('drilldown')).toEqual([['m1'], ['m2']])
  })
})
