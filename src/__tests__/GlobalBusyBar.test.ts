import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setFakeMedia } from './helpers/media-mock'
import GlobalBusyBar from '@/components/GlobalBusyBar.vue'
import { trackBusy, resetGlobalBusy } from '@/composables/globalBusy'

/**
 * 全局忙碌条移动档位置适配（issue #842）：分支断言只落在档位类名上
 * （移动档顶缘随安全区下移；具体像素归 CSS，不测像素值，spec #838 测试决策）。
 * 忙碌态走真实路径：trackBusy + 300ms 阈值（fake timers 推进）。
 */

function deferred<T = unknown>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((res) => {
    resolve = res
  })
  return { promise, resolve }
}

beforeEach(() => {
  vi.useFakeTimers()
  resetGlobalBusy()
})

afterEach(() => {
  vi.useRealTimers()
})

async function mountBusyBar() {
  const d = deferred<void>()
  void trackBusy(d.promise)
  const wrapper = mount(GlobalBusyBar)
  await vi.advanceTimersByTimeAsync(300)
  await flushPromises()
  return { wrapper, resolve: () => d.resolve() }
}

describe('GlobalBusyBar 窗口分级分支（issue #842）', () => {
  it('桌面档：忙碌条渲染，无移动档定位类名', async () => {
    const { wrapper } = await mountBusyBar()
    expect(wrapper.find('.global-busy-bar').exists()).toBe(true)
    expect(wrapper.find('.global-busy-bar.is-mobile-tier').exists()).toBe(false)
  })

  it('缩窗换档：839 → 移动档类名即现；900 → 恢复（定位类名随档位实时切换）', async () => {
    const { wrapper } = await mountBusyBar()
    setFakeMedia({ width: 839 })
    await flushPromises()
    expect(wrapper.find('.global-busy-bar.is-mobile-tier').exists()).toBe(true)
    setFakeMedia({ width: 900 })
    await flushPromises()
    expect(wrapper.find('.global-busy-bar.is-mobile-tier').exists()).toBe(false)
  })
})
