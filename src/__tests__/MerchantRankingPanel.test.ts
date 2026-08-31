import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import MerchantRankingPanel from '@/components/reports/MerchantRankingPanel.vue'
import type { MerchantShare } from '@/types'

/** 夹具即后端返回顺序（净额降序），面板只渲染不再排序；商户已是纯名字行（issue #223）。 */
const shares: MerchantShare[] = [
  { merchant_id: 'm1', merchant_name: '京东', amount_cents: 170000 },
  { merchant_id: 'm2', merchant_name: '红旗连锁', amount_cents: 100000 },
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

  it('纯名称行：不渲染色块与图标文本（icon/color 已退役，issue #223）', () => {
    const wrapper = mount(MerchantRankingPanel, { props: { shares } })
    expect(wrapper.find('[data-testid="merchant-rank-dot"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="merchant-rank-icon"]').exists()).toBe(false)
  })

  it('空排行显示空态提示', () => {
    const wrapper = mount(MerchantRankingPanel, { props: { shares: [] } })
    expect(wrapper.find('[data-testid="merchant-empty"]').exists()).toBe(true)
    expect(wrapper.findAll('[data-testid="merchant-rank-row"]')).toHaveLength(0)
  })
})
