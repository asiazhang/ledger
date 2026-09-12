import { describe, it, expect } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { NSelect } from 'naive-ui'
import GeneralSettings from '@/components/settings/GeneralSettings.vue'

/**
 * GeneralSettings 组件测试（issue #858 币种设置拆分后；issue #930 日志卡片迁入）：本组件
 * 承载轻量设备偏好卡片（ADR-0022 修订后「通用」的应用级偏好主体），展示币种仍是
 * localStorage 权威、不触后端；末位日志卡片（LogSettings）走 get_log_level 接缝。
 */

function selects(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAllComponents(NSelect)
}

describe('GeneralSettings.vue — 展示币种（轻量设备偏好，issue #858）', () => {
  it('渲染展示币种卡片与设备偏好提示，不再含账本级本位币基准', () => {
    wireInvokeSeam({ defaults: { get_log_level: { level: 'info' } } })
    const wrapper = mount(GeneralSettings)
    expect(wrapper.text()).toContain('展示币种')
    expect(wrapper.text()).toContain('不随同步')
    expect(wrapper.text()).not.toContain('本位币基准')
  })

  it('展示币种仍是设备偏好：切换写 localStorage、不触后端命令', async () => {
    wireInvokeSeam({ defaults: { get_log_level: { level: 'info' } } })
    const wrapper = mount(GeneralSettings)
    await flushPromises()
    selects(wrapper)[0].vm.$emit('update:value', 'HKD')
    await flushPromises()
    expect(JSON.parse(localStorage.getItem('default_currency') ?? 'null')).toBe('HKD')
    expect(mockInvoke.mock.calls.filter(([c]) => c === 'set_base_currency')).toHaveLength(0)
  })
})
