import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import AmountCell from '@/components/AmountCell.vue'
import { hasOpenOverlay, resetOverlays } from '@/composables/overlayRegistry'
import { setFakeMedia } from './helpers/media-mock'

/**
 * 金额单元格（issue #843 / ADR-0088 决策 6 悬停一击可达 · 交易表金额全文）：
 * 只测外部行为——指针轴纯 span 零交互（桌面零变化）；触控轴点按弹出全文
 * （经 AppPopover 入弹层注册表）。文案与色的口径归 transaction-columns 列配置
 * （其载荷单点断言见 transaction-columns.test.ts），本组件不持业务语义。
 */

beforeEach(() => {
  resetOverlays()
})

describe('AmountCell 输入轴形态（issue #843 两轴对比）', () => {
  it('指针轴：纯 span 渲染文案与语义色，点击无气泡、无弹层上报', async () => {
    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    const wrapper = mount(AmountCell, { props: { text: '¥1,234.56', color: '#d03050' } })
    const span = wrapper.find('.amount-cell')
    expect(span.exists()).toBe(true)
    expect(span.text()).toBe('¥1,234.56')
    expect(span.attributes('style')).toContain('color: rgb(208, 48, 80)')
    await span.trigger('click')
    expect(document.body.querySelector('.n-popover')).toBeNull()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('触控轴：金额成为点按触发器，点按弹出全文气泡并上报弹层注册表', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = mount(AmountCell, { props: { text: '¥1,234.56', color: '#d03050' } })
    expect(hasOpenOverlay()).toBe(false)
    await wrapper.find('.amount-cell').trigger('click')
    const popover = document.body.querySelector('.n-popover')
    expect(popover).not.toBeNull()
    expect(popover!.textContent).toContain('¥1,234.56')
    expect(hasOpenOverlay()).toBe(true)
  })

  it('触控轴：展示文案与气泡全文同源（同一 formatAmount 产物，含掩码态）', async () => {
    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    const wrapper = mount(AmountCell, { props: { text: '••••', color: '#000' } })
    expect(wrapper.find('.amount-cell').text()).toBe('••••')
    await wrapper.find('.amount-cell').trigger('click')
    expect(document.body.querySelector('.n-popover')!.textContent).toContain('••••')
  })
})
