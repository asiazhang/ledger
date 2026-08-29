import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import MerchantRankingPanel from '@/components/reports/MerchantRankingPanel.vue'
import type { MerchantShare } from '@/types'

/** 夹具即后端返回顺序（净额降序），面板只渲染不再排序。 */
const shares: MerchantShare[] = [
  { merchant_id: 'm1', merchant_name: '京东', icon: '京东标', color: '#e37318', amount_cents: 170000 },
  { merchant_id: 'm2', merchant_name: '红旗连锁', icon: null, color: null, amount_cents: 100000 },
]

describe('MerchantRankingPanel', () => {
  it('按传入顺序渲染商户行与净支出金额（本位币分格式化）', () => {
    const wrapper = mount(MerchantRankingPanel, { props: { shares } })
    const rows = wrapper.findAll('[data-testid="merchant-rank-row"]')
    expect(rows).toHaveLength(2)
    expect(rows[0].find('[data-testid="merchant-rank-name"]').text()).toBe('京东')
    expect(rows[0].find('[data-testid="merchant-rank-amount"]').text()).toContain('1700')
    expect(rows[1].find('[data-testid="merchant-rank-name"]').text()).toBe('红旗连锁')
    expect(rows[1].find('[data-testid="merchant-rank-amount"]').text()).toContain('1000')
  })

  it('附带 icon/color 视觉辨识：色块着色、icon 文本展示，缺省时隐藏', () => {
    const wrapper = mount(MerchantRankingPanel, { props: { shares } })
    const rows = wrapper.findAll('[data-testid="merchant-rank-row"]')
    // 第一行：color 色块 + icon 文本
    const dot0 = rows[0].find('[data-testid="merchant-rank-dot"]')
    expect(dot0.exists()).toBe(true)
    expect((dot0.element as HTMLElement).style.backgroundColor).toBe('rgb(227, 115, 24)')
    expect(rows[0].find('[data-testid="merchant-rank-icon"]').text()).toBe('京东标')
    // 第二行：无 color/icon 不渲染对应元素
    expect(rows[1].find('[data-testid="merchant-rank-dot"]').exists()).toBe(false)
    expect(rows[1].find('[data-testid="merchant-rank-icon"]').exists()).toBe(false)
  })

  it('空排行显示空态提示', () => {
    const wrapper = mount(MerchantRankingPanel, { props: { shares: [] } })
    expect(wrapper.find('[data-testid="merchant-empty"]').exists()).toBe(true)
    expect(wrapper.findAll('[data-testid="merchant-rank-row"]')).toHaveLength(0)
  })
})
