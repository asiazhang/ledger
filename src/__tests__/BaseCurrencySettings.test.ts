import { describe, it, expect } from 'vitest'
import { mockInvoke, wireInvokeSeam, lastInvokeArgs } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import BaseCurrencySettings from '@/components/settings/BaseCurrencySettings.vue'

/**
 * 本位币基准卡片组件测试（issue #858）：invoke 测试接缝布线（ADR-0085），
 * defaults 表给命令契约静态快照；卡片是「通用」页签轻量偏好之外的唯一
 * 账本级设置（ADR-0022 领域归属 → 「分类」页签）。
 */

function mountCard() {
  return mount(BaseCurrencySettings)
}

describe('BaseCurrencySettings.vue — 本位币基准（账本级设置，issue #858）', () => {
  it('渲染本位币基准卡片与同步提示', () => {
    wireInvokeSeam({ defaults: { get_base_currency: { code: 'CNY' } } })
    const wrapper = mountCard()
    expect(wrapper.text()).toContain('本位币基准')
    expect(wrapper.text()).toContain('同步')
  })

  it('挂载经 get_base_currency 读回显（不走 localStorage）', async () => {
    wireInvokeSeam({ defaults: { get_base_currency: { code: 'USD' } } })
    const wrapper = mountCard()
    await flushPromises()
    expect(mockInvoke.mock.calls.some(([c]) => c === 'get_base_currency')).toBe(true)
    expect(wrapper.findComponent(NSelect).props('value')).toBe('USD')
  })

  it('切换基准下拉调 set_base_currency 并以命令返回值回写', async () => {
    wireInvokeSeam({
      defaults: { get_base_currency: { code: 'CNY' } },
      overrides: { set_base_currency: { code: 'EUR' } },
    })
    const wrapper = mountCard()
    await flushPromises()
    wrapper.findComponent(NSelect).vm.$emit('update:value', 'EUR')
    await flushPromises()
    expect(lastInvokeArgs('set_base_currency')).toEqual({ code: 'EUR' })
    expect(wrapper.findComponent(NSelect).props('value')).toBe('EUR')
  })
})
