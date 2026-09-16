import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import InstrumentLink from '@/investment/InstrumentLink.vue'
import { useAppStore } from '@/stores/app'

// 标的下钻经 useRouter（AccountLink/MerchantLink 同款 pushMock 断言先例）
const pushMock = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push: pushMock }),
}))

beforeEach(() => {
  pushMock.mockReset()
})

describe('InstrumentLink 标的前提下钻（ADR-0107）强调色（issue #1268）', () => {
  it('暗色主题（默认）：主题强调色琥珀 + hover 变量亮琥珀', () => {
    useAppStore().setTheme('dark')
    const wrapper = mount(InstrumentLink, {
      props: { instrumentId: 'inst-1', label: '600519' },
    })
    const style = wrapper.find('button').attributes('style')
    expect(style).toContain('rgb(245, 158, 11)')
    expect(style).toContain('--accent-hover: #FBBF24')
  })

  it('亮色主题：强调色切同色相加深版（#B45309 / hover #92400E）', () => {
    useAppStore().setTheme('light')
    const wrapper = mount(InstrumentLink, {
      props: { instrumentId: 'inst-1', label: '600519' },
    })
    const style = wrapper.find('button').attributes('style')
    expect(style).toContain('rgb(180, 83, 9)')
    expect(style).toContain('--accent-hover: #92400E')
  })

  it('label 为空渲染纯文本「-」，无按钮、无强调色', () => {
    const wrapper = mount(InstrumentLink, {
      props: { instrumentId: 'inst-1', label: null },
    })
    expect(wrapper.find('button').exists()).toBe(false)
    expect(wrapper.find('span').text()).toBe('-')
  })
})
