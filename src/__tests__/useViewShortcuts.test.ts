import { describe, it, expect, vi, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount } from '@vue/test-utils'
import type { Router } from 'vue-router'
import {
  viewShortcuts,
  matchViewShortcut,
  shortcutHint,
  hasOpenOverlay,
  useViewShortcuts,
} from '@/composables/useViewShortcuts'

function setPlatform(platform: string) {
  Object.defineProperty(navigator, 'platform', { value: platform, configurable: true })
}

function press(key: string, mods: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean } = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key, bubbles: true, ...mods })
}

afterEach(() => setPlatform(''))

describe('viewShortcuts 映射', () => {
  it('按菜单顺序覆盖 9 个视图，序号 1..9 连续无空洞', () => {
    expect(viewShortcuts.map((s) => s.key)).toEqual(['1', '2', '3', '4', '5', '6', '7', '8', '9'])
    expect(viewShortcuts.map((s) => s.name)).toEqual([
      'dashboard',
      'transactions',
      'search',
      'accounts',
      'reports',
      'investments',
      'budget',
      'ai',
      'settings',
    ])
  })
})

describe('matchViewShortcut', () => {
  it('macOS 上 Cmd+数字命中对应视图', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1', { metaKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('2', { metaKey: true }))).toBe('transactions')
    expect(matchViewShortcut(press('9', { metaKey: true }))).toBe('settings')
  })

  it('macOS 上 Ctrl+数字不命中（需要 Cmd）', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBeNull()
  })

  it('非 macOS 上 Ctrl+数字命中对应视图', () => {
    setPlatform('Win32')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('3', { ctrlKey: true }))).toBe('search')
    expect(matchViewShortcut(press('7', { ctrlKey: true }))).toBe('budget')
  })

  it('非 macOS 上 Cmd+数字不命中（需要 Ctrl）', () => {
    setPlatform('Win32')
    expect(matchViewShortcut(press('1', { metaKey: true }))).toBeNull()
  })

  it('无修饰键 / 混按 Cmd+Ctrl / Shift / Alt / 非 1..9 键均不命中', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1'))).toBeNull()
    expect(matchViewShortcut(press('1', { metaKey: true, ctrlKey: true }))).toBeNull()
    expect(matchViewShortcut(press('1', { metaKey: true, shiftKey: true }))).toBeNull()
    expect(matchViewShortcut(press('1', { metaKey: true, altKey: true }))).toBeNull()
    expect(matchViewShortcut(press('0', { metaKey: true }))).toBeNull()
    expect(matchViewShortcut(press('a', { metaKey: true }))).toBeNull()
  })
})

describe('shortcutHint', () => {
  it('macOS 显示 ⌘N，其余显示 Ctrl+N', () => {
    setPlatform('MacIntel')
    expect(shortcutHint('1')).toBe('⌘1')
    setPlatform('Win32')
    expect(shortcutHint('1')).toBe('Ctrl+1')
  })
})

describe('hasOpenOverlay', () => {
  it('无覆盖层时返回 false', () => {
    expect(hasOpenOverlay()).toBe(false)
  })

  it('检测 NModal / NDialog / NPopconfirm 覆盖层', () => {
    const el = document.createElement('div')
    el.className = 'n-modal-container'
    document.body.appendChild(el)
    expect(hasOpenOverlay()).toBe(true)
    el.remove()
    expect(hasOpenOverlay()).toBe(false)

    const dialog = document.createElement('div')
    dialog.className = 'n-dialog'
    document.body.appendChild(dialog)
    expect(hasOpenOverlay()).toBe(true)
    dialog.remove()

    const popconfirm = document.createElement('div')
    popconfirm.className = 'n-popconfirm'
    document.body.appendChild(popconfirm)
    expect(hasOpenOverlay()).toBe(true)
    popconfirm.remove()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('检测下拉菜单 / 筛选下拉弹层（issue #153 扩展）', () => {
    const dropdown = document.createElement('div')
    dropdown.className = 'n-dropdown-menu'
    document.body.appendChild(dropdown)
    expect(hasOpenOverlay()).toBe(true)
    dropdown.remove()
    expect(hasOpenOverlay()).toBe(false)

    const selectMenu = document.createElement('div')
    selectMenu.className = 'n-base-select-menu'
    document.body.appendChild(selectMenu)
    expect(hasOpenOverlay()).toBe(true)
    selectMenu.remove()
    expect(hasOpenOverlay()).toBe(false)
  })
})

function makeRouter(initialName: string) {
  const state = { name: initialName }
  const push = vi.fn().mockImplementation((to: { name: string }) => {
    state.name = to.name
  })
  const router = {
    currentRoute: { value: { name: state.name } },
    push,
  } as unknown as Router
  return { router, push, get name() { return state.name } }
}

function mountHost(router: Router) {
  const Host = defineComponent({
    setup() {
      useViewShortcuts(router)
      return () => h('div')
    },
  })
  return mount(Host)
}

describe('useViewShortcuts', () => {
  it('命中快捷键时跳转到目标路由', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).toHaveBeenCalledWith({ name: 'accounts' })
  })

  it('已在目标路由时不重复跳转', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('accounts')
    mountHost(router)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
  })

  it('覆盖层打开时抑制跳转', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    const overlay = document.createElement('div')
    overlay.className = 'n-modal-container'
    document.body.appendChild(overlay)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
    overlay.remove()
  })

  it('未命中快捷键时不跳转也不报错', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    window.dispatchEvent(press('4', { ctrlKey: true }))
    window.dispatchEvent(press('0', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
  })
})
