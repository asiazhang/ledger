import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import MerchantRankingPanel from '@/components/reports/MerchantRankingPanel.vue'
import { useReferenceStore } from '@/stores/reference'
import { paletteColor } from '@/utils/category-chart'
import type { Merchant } from '@/types'
import type { MerchantSharesReport } from '@/types'

// 商户消费排行表格（issue #618 表格化）：面板只做渲染与交互接线——列渲染、
// 点商户名上报下钻意图、TopN 档位切换、空态；比例 / 占比 / 负值处理等口径
// 全部收口 merchantTableRows 纯函数（merchant-chart.test.ts 锁口径）。
// 排序与 topN 截断收口后端，面板按返回序渲染、无排名序号列。

const mockMerchants: Merchant[] = [
  { id: 'm1', name: '京东', is_deleted: false, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test' },
  { id: 'm2', name: '红旗连锁', is_deleted: false, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test' },
  { id: 'm3', name: '退款户', is_deleted: false, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test' },
]

/** 夹具即后端返回顺序（净额降序 + topN 截断已收口后端），面板只渲染不再排序。 */
const rows = [
  { merchant_id: 'm1', merchant_name: '京东', amount_cents: 170000, transaction_count: 9 },
  { merchant_id: 'm2', merchant_name: '红旗连锁', amount_cents: 85000, transaction_count: 5 },
  { merchant_id: 'm3', merchant_name: '退款户', amount_cents: -5000, transaction_count: 1 },
]
// total_cents 刻意 ≠ rows 合计（250000）：占比分母断言可辨真源
const report: MerchantSharesReport = { rows, total_cents: 340000 }

function mountPanel(props: { report?: MerchantSharesReport; topN?: number } = {}) {
  const pinia = createPinia()
  setActivePinia(pinia)
  useReferenceStore().merchants = mockMerchants
  return mount(MerchantRankingPanel, {
    props: { report: props.report ?? report, topN: props.topN ?? 5 },
    global: { plugins: [pinia] },
  })
}

type Wrapper = ReturnType<typeof mountPanel>

/** 表格数据行（naive-ui NDataTable 渲染的 tbody tr） */
function bodyRows(wrapper: Wrapper) {
  return wrapper.findAll('[data-testid="merchant-table"] tbody tr')
}

type RowWrapper = ReturnType<typeof bodyRows>[number]

/** 行内按 data-testid 取单元格文本 */
function cellText(row: RowWrapper, testid: string): string {
  return row.find(`[data-testid="${testid}"]`).text()
}

describe('MerchantRankingPanel 表格化（issue #618）', () => {
  it('五列表头齐备且无排名序号列：商户名 | 金额分布 | 金额 | 占比 | 笔数', () => {
    const wrapper = mountPanel()
    const headers = wrapper
      .findAll('[data-testid="merchant-table"] thead th')
      .map((th) => th.text().trim())
    expect(headers).toEqual(['商户名', '金额分布', '金额', '占比', '笔数'])
  })

  it('行序 = 后端返回序：商户名、金额数字、占比%、笔数逐行渲染', () => {
    const wrapper = mountPanel()
    const trs = bodyRows(wrapper)
    expect(trs).toHaveLength(3)
    expect(cellText(trs[0], 'merchant-name')).toBe('京东')
    expect(cellText(trs[1], 'merchant-name')).toBe('红旗连锁')
    expect(cellText(trs[2], 'merchant-name')).toBe('退款户')
    // 金额走 formatAmount（分 → 元）：170000 → 1700、-5000 → -50
    expect(cellText(trs[0], 'merchant-amount')).toBe('1700')
    expect(cellText(trs[1], 'merchant-amount')).toBe('850')
    expect(cellText(trs[2], 'merchant-amount')).toBe('-50')
    // 占比分母 = 载荷全量合计 340000：170000 → 50%、85000 → 25%、-5000 → -1%
    //（误用展示行合计 250000 会得 68%/34%）
    expect(cellText(trs[0], 'merchant-share')).toBe('50%')
    expect(cellText(trs[1], 'merchant-share')).toBe('25%')
    expect(cellText(trs[2], 'merchant-share')).toBe('-1%')
    expect(cellText(trs[0], 'merchant-count')).toBe('9')
    expect(cellText(trs[1], 'merchant-count')).toBe('5')
    expect(cellText(trs[2], 'merchant-count')).toBe('1')
  })

  it('内嵌条：条长 = 金额 ÷ 显示区最大金额（width 100%/50%），配色按名次取色', () => {
    const wrapper = mountPanel()
    const trs = bodyRows(wrapper)
    const fill0 = trs[0].find('[data-testid="merchant-bar"]')
    const fill1 = trs[1].find('[data-testid="merchant-bar"]')
    expect(fill0.attributes('style')).toContain('width: 100%')
    expect(fill1.attributes('style')).toContain('width: 50%')
    // 名次色内联注入（分类色板，第 1 名 = 色板首位）；jsdom 将 hex 序列化为 rgb
    const rgb = (hex: string) => {
      const n = parseInt(hex.slice(1), 16)
      return `rgb(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255})`
    }
    expect(fill0.attributes('style')).toContain(rgb(paletteColor(0)))
    expect(fill1.attributes('style')).toContain(rgb(paletteColor(1)))
  })

  it('负净额行不画条（width 0%），金额与占比照实显示', () => {
    const wrapper = mountPanel()
    const fill = bodyRows(wrapper)[2].find('[data-testid="merchant-bar"]')
    expect(fill.attributes('style')).toContain('width: 0%')
    expect(cellText(bodyRows(wrapper)[2], 'merchant-amount')).toBe('-50')
    expect(cellText(bodyRows(wrapper)[2], 'merchant-share')).toBe('-1%')
  })

  it('点商户名上报下钻意图：emit drilldown（携带 merchant_id，载荷归报表视图构造）', async () => {
    const wrapper = mountPanel()
    await bodyRows(wrapper)[0].find('[data-testid="merchant-name"]').trigger('click')
    expect(wrapper.emitted('drilldown')).toEqual([['m1']])
    await bodyRows(wrapper)[1].find('[data-testid="merchant-name"]').trigger('click')
    expect(wrapper.emitted('drilldown')).toEqual([['m1'], ['m2']])
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

  it('空排行显示空态提示（NEmpty 保留），不渲染表格', () => {
    const wrapper = mountPanel({ report: { rows: [], total_cents: 0 } })
    expect(wrapper.find('[data-testid="merchant-empty"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="merchant-table"]').exists()).toBe(false)
  })
})
