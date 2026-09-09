import { describe, it, expect, vi, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { fireViewReset, registerViewReset, clearViewResets } from '@/composables/viewResetRegistry'

/**
 * 复位回调注册表（spec #892 唯一新缝）测试：只测外部行为——注册/撤销/消费，
 * 不断言内部实现。注册表不持业务语义：回调做什么由注册方决定。
 */

const wrappers: VueWrapper[] = []
afterEach(() => {
  while (wrappers.length) wrappers.pop()?.unmount()
  clearViewResets()
})

/** 在组件作用域内注册复位回调的宿主（视图注册形态同构）。 */
function mountHost(reset: () => void) {
  const Host = defineComponent({
    setup() {
      registerViewReset(reset)
      return () => h('div')
    },
  })
  const wrapper = mount(Host)
  wrappers.push(wrapper)
  return wrapper
}

describe('viewResetRegistry（spec #892 复位回调注册点）', () => {
  it('注册后可消费：fireViewReset 执行回调并返回 true', () => {
    const reset = vi.fn()
    mountHost(reset)
    expect(fireViewReset()).toBe(true)
    expect(reset).toHaveBeenCalledTimes(1)
  })

  it('无注册时消费无操作：返回 false、不报错（无保留状态的视图不注册即天然无操作）', () => {
    expect(fireViewReset()).toBe(false)
  })

  it('作用域销毁自动撤销：组件卸载后消费不再触达回调（导航离开/跨断点换档卸载兜底）', () => {
    const reset = vi.fn()
    const host = mountHost(reset)
    host.unmount()
    expect(fireViewReset()).toBe(false)
    expect(reset).not.toHaveBeenCalled()
  })

  it('重复注册以最后一次为准（单槽：同一时刻至多一个视图挂载，无 KeepAlive）', () => {
    const first = vi.fn()
    const second = vi.fn()
    mountHost(first)
    mountHost(second)
    fireViewReset()
    expect(first).not.toHaveBeenCalled()
    expect(second).toHaveBeenCalledTimes(1)
    // 先注册的视图卸载不撤销后注册的回调（注册表按「当前注册」判定，不误清）
    wrappers[0]!.unmount()
    expect(fireViewReset()).toBe(true)
    expect(second).toHaveBeenCalledTimes(2)
  })

  it('clearViewResets 清空注册表（测试辅助，模拟整体卸载后的干净状态）', () => {
    const reset = vi.fn()
    mountHost(reset)
    clearViewResets()
    expect(fireViewReset()).toBe(false)
    expect(reset).not.toHaveBeenCalled()
  })
})
