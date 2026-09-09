import { describe, it, expect, vi, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { useWindowGuard } from '@/composables/useWindowGuard'
import { createOverlayToken, resetOverlays } from '@/composables/overlayRegistry'
import { registerViewReset, clearViewResets } from '@/composables/viewResetRegistry'

/**
 * 窗口行为守卫（issue #154）测试：只测外部行为——
 * 向 document 派发真实事件，断言 defaultPrevented 与事件继续传播
 * （未被 stopPropagation），不断言模块内部实现。
 */

const wrappers: VueWrapper[] = []
afterEach(() => {
  while (wrappers.length) wrappers.pop()?.unmount()
  resetOverlays()
  clearViewResets()
})

function mountHost() {
  const Host = defineComponent({
    setup() {
      useWindowGuard()
      return () => h('div')
    },
  })
  const wrapper = mount(Host)
  wrappers.push(wrapper)
  return wrapper
}

/** 在指定元素上派发可取消事件，返回该事件供断言。 */
function fire(target: EventTarget, type: string): Event {
  const e = new Event(type, { bubbles: true, cancelable: true })
  target.dispatchEvent(e)
  return e
}

/** 收集事件继续冒泡到 window 的监听（未被 stopPropagation 时被调用）。 */
function bubbleSpy() {
  return vi.fn<(e: Event) => void>()
}

describe('useWindowGuard：Escape 拦截', () => {
  it('ESC 按键被 preventDefault（任何状态下不作用于窗口层，不退出全屏）', () => {
    mountHost()
    const esc = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    document.body.dispatchEvent(esc)
    expect(esc.defaultPrevented).toBe(true)
  })

  it('焦点在可编辑元素上 ESC 仍被拦截（无条件，不区分状态）', () => {
    mountHost()
    const input = document.createElement('input')
    document.body.appendChild(input)
    const esc = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    input.dispatchEvent(esc)
    expect(esc.defaultPrevented).toBe(true)
    input.remove()
  })

  it('拦截只 preventDefault 不阻断传播：事件仍冒泡到 window（naive-ui 弹层依赖）', () => {
    mountHost()
    const spy = bubbleSpy()
    window.addEventListener('keydown', spy)
    const esc = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    document.body.dispatchEvent(esc)
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toBe(esc)
    window.removeEventListener('keydown', spy)
  })

  it('非 ESC 按键不受影响', () => {
    mountHost()
    const a = new KeyboardEvent('keydown', { key: 'a', bubbles: true, cancelable: true })
    document.body.dispatchEvent(a)
    expect(a.defaultPrevented).toBe(false)
    const enter = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })
    document.body.dispatchEvent(enter)
    expect(enter.defaultPrevented).toBe(false)
  })
})

// —— ESC 两级语义（spec #892 / ADR-0094）：有弹层时弹层库默认行为接管，无弹层时
// 消费当前视图注册的复位回调。判定复用弹层注册表（ADR-0035），不做 DOM 推断。

describe('useWindowGuard：ESC 两级语义（spec #892）', () => {
  /** 带注册回调的宿主（模拟持有保留态的视图；守卫与注册宿主分离，
   * 真实应用中守卫只在 App.vue 挂载一次）。 */
  function mountHostWithReset(reset: () => void, withGuard = true) {
    const Host = defineComponent({
      setup() {
        if (withGuard) useWindowGuard()
        registerViewReset(reset)
        return () => h('div')
      },
    })
    const wrapper = mount(Host)
    wrappers.push(wrapper)
    return wrapper
  }

  function fireEscape() {
    const esc = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    document.body.dispatchEvent(esc)
    return esc
  }

  it('无弹层 + 有注册回调：ESC 触发复位回调（仍 preventDefault，不作用于窗口层）', () => {
    const reset = vi.fn()
    mountHostWithReset(reset)
    const esc = fireEscape()
    expect(esc.defaultPrevented).toBe(true)
    expect(reset).toHaveBeenCalledTimes(1)
  })

  it('有弹层：ESC 不消费复位回调（弹层库默认关闭行为接管，一次按键只做一件事）', () => {
    const reset = vi.fn()
    mountHostWithReset(reset)
    const token = createOverlayToken('modal')
    token.set(true)
    fireEscape()
    expect(reset).not.toHaveBeenCalled()
    token.set(false)
    // 弹层关再按：复位通道恢复
    fireEscape()
    expect(reset).toHaveBeenCalledTimes(1)
  })

  it('无注册回调：ESC 无操作（不报错，无保留状态的视图天然无操作）', () => {
    mountHost()
    expect(() => fireEscape()).not.toThrow()
  })

  it('视图卸载后注册自动撤销：ESC 不再触发（不滞留注册态）', () => {
    const reset = vi.fn()
    const host = mountHostWithReset(reset)
    fireEscape()
    expect(reset).toHaveBeenCalledTimes(1)
    host.unmount()
    fireEscape()
    expect(reset).toHaveBeenCalledTimes(1)
  })

  it('多视图先后注册：以最后一次注册为准（当前视图的复位回调），先注册者卸载不误清', () => {
    const first = vi.fn()
    const second = vi.fn()
    mountHost() // 守卫仅一处
    mountHostWithReset(first, false)
    mountHostWithReset(second, false)
    fireEscape()
    expect(first).not.toHaveBeenCalled()
    expect(second).toHaveBeenCalledTimes(1)
    // 先注册的视图卸载不撤销后注册的回调（注册表按「当前注册」判定，不误清）
    wrappers[1]!.unmount()
    fireEscape()
    expect(second).toHaveBeenCalledTimes(2)
  })
})

describe('useWindowGuard：原生右键菜单拦截', () => {
  it('非可编辑区域 contextmenu 被 preventDefault（不弹 WKWebView 默认菜单）', () => {
    mountHost()
    const e = fire(document.body, 'contextmenu')
    expect(e.defaultPrevented).toBe(true)
    const div = document.createElement('div')
    document.body.appendChild(div)
    expect(fire(div, 'contextmenu').defaultPrevented).toBe(true)
    div.remove()
  })

  it.each(['input', 'textarea', 'select'])('可编辑元素 <%s> 内 contextmenu 放行（保留系统编辑菜单）', (tag) => {
    mountHost()
    const el = document.createElement(tag)
    document.body.appendChild(el)
    const e = fire(el, 'contextmenu')
    expect(e.defaultPrevented).toBe(false)
    el.remove()
  })

  it('contenteditable 元素内 contextmenu 放行', () => {
    mountHost()
    const el = document.createElement('div')
    el.setAttribute('contenteditable', '')
    document.body.appendChild(el)
    expect(fire(el, 'contextmenu').defaultPrevented).toBe(false)
    el.remove()
  })

  it('拦截只 preventDefault 不阻断传播：行级自定义右键菜单仍可读取事件（坐标等）', () => {
    mountHost()
    const spy = bubbleSpy()
    window.addEventListener('contextmenu', spy)
    const e = fire(document.body, 'contextmenu')
    expect(e.defaultPrevented).toBe(true)
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toBe(e)
    window.removeEventListener('contextmenu', spy)
  })
})

describe('useWindowGuard：生命周期', () => {
  it('卸载后不再拦截（随应用根组件装卸）', () => {
    const wrapper = mountHost()
    wrapper.unmount()
    expect(fire(document.body, 'contextmenu').defaultPrevented).toBe(false)
    const esc = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    document.body.dispatchEvent(esc)
    expect(esc.defaultPrevented).toBe(false)
  })

  it('拦截注册在 document 捕获阶段：先于目标层既有处理器生效', () => {
    // 行级处理器（如 TransactionsView rowProps 的 onContextmenu）在目标/冒泡阶段，
    // 守卫必须能先于其执行且不影响其收到事件
    mountHost()
    const row = document.createElement('div')
    document.body.appendChild(row)
    const rowHandler = bubbleSpy()
    row.addEventListener('contextmenu', rowHandler)
    const e = fire(row, 'contextmenu')
    expect(e.defaultPrevented).toBe(true)
    expect(rowHandler).toHaveBeenCalledTimes(1)
    row.remove()
  })
})
