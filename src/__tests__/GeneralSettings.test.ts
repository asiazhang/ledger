import { describe, it, expect } from 'vitest'
import { mockInvoke, wireInvokeSeam, lastInvokeArgs } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import GeneralSettings from '@/components/settings/GeneralSettings.vue'

/**
 * GeneralSettings 组件测试（issue #858）：invoke 测试接缝布线（ADR-0085），
 * defaults 表给命令契约静态快照；两枚币种下拉的组件事实以 NSelect 顺序锚定
 * （模板顺序 = 本位币基准在前、展示币种在后）。
 */

function selects(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAllComponents(NSelect)
}

describe('GeneralSettings.vue — 本位币基准（账本级设置，issue #858）', () => {
  it('渲染本位币基准与展示币种两张卡片，基准卡片带同步提示', () => {
    wireInvokeSeam({ defaults: { get_base_currency: { code: 'CNY' } } })
    const wrapper = mount(GeneralSettings)
    expect(wrapper.text()).toContain('本位币基准')
    expect(wrapper.text()).toContain('展示币种')
    expect(wrapper.text()).toContain('同步')
  })

  it('挂载经 get_base_currency 读回显（不走 localStorage）', async () => {
    wireInvokeSeam({ defaults: { get_base_currency: { code: 'USD' } } })
    const wrapper = mount(GeneralSettings)
    await flushPromises()
    expect(mockInvoke.mock.calls.some(([c]) => c === 'get_base_currency')).toBe(true)
    expect(selects(wrapper)[0].props('value')).toBe('USD')
  })

  it('切换基准下拉调 set_base_currency 并以命令返回值回写', async () => {
    wireInvokeSeam({
      defaults: { get_base_currency: { code: 'CNY' } },
      overrides: { set_base_currency: { code: 'EUR' } },
    })
    const wrapper = mount(GeneralSettings)
    await flushPromises()
    selects(wrapper)[0].vm.$emit('update:value', 'EUR')
    await flushPromises()
    expect(lastInvokeArgs('set_base_currency')).toEqual({ code: 'EUR' })
    expect(selects(wrapper)[0].props('value')).toBe('EUR')
  })

  it('展示币种仍是设备偏好：切换写 localStorage、不触后端命令', async () => {
    wireInvokeSeam({ defaults: { get_base_currency: { code: 'CNY' } } })
    const wrapper = mount(GeneralSettings)
    await flushPromises()
    selects(wrapper)[1].vm.$emit('update:value', 'HKD')
    await flushPromises()
    expect(JSON.parse(localStorage.getItem('default_currency') ?? 'null')).toBe('HKD')
    expect(mockInvoke.mock.calls.filter(([c]) => c === 'set_base_currency')).toHaveLength(0)
  })
})
