import { describe, it, expect, vi, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount } from '@vue/test-utils'
import type { Router } from 'vue-router'
import {
  DEFAULT_VIEW_ORDER,
  ARRANGEABLE_VIEWS,
  FIRST_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
  viewShortcuts,
  parseArrangeableOrder,
  matchViewShortcut,
  shortcutHint,
  hasOpenOverlay,
  useViewShortcuts,
} from '@/composables/useViewShortcuts'
import { VIEW_STATE_KEYS } from '@/utils/view-state'

function setPlatform(platform: string) {
  Object.defineProperty(navigator, 'platform', { value: platform, configurable: true })
}

function press(key: string, mods: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean } = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key, bubbles: true, ...mods })
}

afterEach(() => setPlatform(''))

describe('顺序模块常量（issue #269）', () => {
  it('默认序锁定：概览、交易、账户、预算、投资、报表、定时、物品、搜索、AI、设置（按使用频率重排）', () => {
    expect([...DEFAULT_VIEW_ORDER]).toEqual([
      'dashboard',
      'transactions',
      'accounts',
      'budget',
      'investments',
      'reports',
      'scheduled',
      'items',
      'search',
      'ai',
      'settings',
    ])
  })

  it('三固定约束：概览首位、AI 倒数第二、设置末位', () => {
    expect(FIRST_VIEW).toBe('dashboard')
    expect(PENULTIMATE_VIEW).toBe('ai')
    expect(LAST_VIEW).toBe('settings')
    expect(DEFAULT_VIEW_ORDER[0]).toBe(FIRST_VIEW)
    expect(DEFAULT_VIEW_ORDER[DEFAULT_VIEW_ORDER.length - 2]).toBe(PENULTIMATE_VIEW)
    expect(DEFAULT_VIEW_ORDER[DEFAULT_VIEW_ORDER.length - 1]).toBe(LAST_VIEW)
  })

  it('可排区 = 第 2–9 位共 8 项：默认序去掉三固定项，相对顺序不变', () => {
    expect(ARRANGEABLE_VIEWS).toEqual([
      'transactions',
      'accounts',
      'budget',
      'investments',
      'reports',
      'scheduled',
      'items',
      'search',
    ])
  })
})

describe('parseArrangeableOrder（顺序解析纯函数）', () => {
  it('空值 / 非数组 / 空数组整体回退默认可排区序', () => {
    const fallback = [...ARRANGEABLE_VIEWS]
    expect(parseArrangeableOrder(null)).toEqual(fallback)
    expect(parseArrangeableOrder(undefined)).toEqual(fallback)
    expect(parseArrangeableOrder('junk')).toEqual(fallback)
    expect(parseArrangeableOrder(123)).toEqual(fallback)
    expect(parseArrangeableOrder({})).toEqual(fallback)
    expect(parseArrangeableOrder([])).toEqual(fallback)
  })

  it('非法视图名被过滤（含三固定项名、未知名、非字符串项），缺失项按默认序补入末尾', () => {
    expect(parseArrangeableOrder(['transactions', 'dashboard', 'bogus', 42, 'accounts', 'settings', 'ai'])).toEqual([
      'transactions',
      'accounts',
      'budget',
      'investments',
      'reports',
      'scheduled',
      'items',
      'search',
    ])
  })

  it('重复项去重保留首现', () => {
    expect(parseArrangeableOrder(['search', 'reports', 'search', 'items'])).toEqual([
      'search',
      'reports',
      'items',
      'transactions',
      'accounts',
      'budget',
      'investments',
      'scheduled',
    ])
  })

  it('缺失项按默认序补入可排区末尾（保留已存相对顺序）', () => {
    expect(parseArrangeableOrder(['reports', 'transactions'])).toEqual([
      'reports',
      'transactions',
      'accounts',
      'budget',
      'investments',
      'scheduled',
      'items',
      'search',
    ])
  })

  it('完整排列原样返回', () => {
    const full = ['search', 'items', 'scheduled', 'reports', 'investments', 'budget', 'accounts', 'transactions']
    expect(parseArrangeableOrder(full)).toEqual(full)
  })
})

describe('viewShortcuts 派生（键位按最终位置推导）', () => {
  it('按菜单位置覆盖全部视图，数字键 1..0 连续无空洞', () => {
    expect(viewShortcuts.value.map((s) => s.key)).toEqual(['1', '2', '3', '4', '5', '6', '7', '8', '9', '0', ','])
    expect(viewShortcuts.value.map((s) => s.name)).toEqual([...DEFAULT_VIEW_ORDER])
  })

  it('每个侧栏视图恰一键；设置用 Cmd+,（macOS 惯例）而非数字位', () => {
    const names = viewShortcuts.value.map((s) => s.name)
    expect(new Set(names).size).toBe(names.length)
    expect(new Set(names)).toEqual(new Set(DEFAULT_VIEW_ORDER))
    expect(viewShortcuts.value.find((v) => v.name === 'settings')!.key).toBe(',')
    expect(viewShortcuts.value.find((v) => v.name === 'scheduled')!.key).toBe('7')
    expect(viewShortcuts.value.find((v) => v.name === 'items')!.key).toBe('8')
  })
})

describe('matchViewShortcut', () => {
  it('macOS 上 Cmd+数字命中对应视图（键位严格随新默认序位置）', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1', { metaKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('2', { metaKey: true }))).toBe('transactions')
    expect(matchViewShortcut(press('3', { metaKey: true }))).toBe('accounts')
    expect(matchViewShortcut(press('4', { metaKey: true }))).toBe('budget')
    expect(matchViewShortcut(press('5', { metaKey: true }))).toBe('investments')
    expect(matchViewShortcut(press('6', { metaKey: true }))).toBe('reports')
    expect(matchViewShortcut(press('7', { metaKey: true }))).toBe('scheduled')
    expect(matchViewShortcut(press('8', { metaKey: true }))).toBe('items')
    expect(matchViewShortcut(press('9', { metaKey: true }))).toBe('search')
    expect(matchViewShortcut(press('0', { metaKey: true }))).toBe('ai')
    expect(matchViewShortcut(press(',', { metaKey: true }))).toBe('settings')
  })

  it('macOS 上 Ctrl+数字不命中（需要 Cmd）', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBeNull()
  })

  it('非 macOS 上 Ctrl+数字命中对应视图', () => {
    setPlatform('Win32')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('3', { ctrlKey: true }))).toBe('accounts')
    expect(matchViewShortcut(press('6', { ctrlKey: true }))).toBe('reports')
    expect(matchViewShortcut(press(',', { ctrlKey: true }))).toBe('settings')
  })

  it('非 macOS 上 Cmd+数字不命中（需要 Ctrl）', () => {
    setPlatform('Win32')
    expect(matchViewShortcut(press('1', { metaKey: true }))).toBeNull()
  })

  it('无修饰键 / 混按 Cmd+Ctrl / Shift / Alt / 未映射键均不命中', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1'))).toBeNull()
    expect(matchViewShortcut(press('1', { metaKey: true, ctrlKey: true }))).toBeNull()
    expect(matchViewShortcut(press('1', { metaKey: true, shiftKey: true }))).toBeNull()
    expect(matchViewShortcut(press('1', { metaKey: true, altKey: true }))).toBeNull()
    expect(matchViewShortcut(press('x', { metaKey: true }))).toBeNull()
    expect(matchViewShortcut(press('a', { metaKey: true }))).toBeNull()
  })
})

describe('shortcutHint', () => {
  it('macOS 显示 ⌘N，其余显示 Ctrl+N；逗号键同样适用', () => {
    setPlatform('MacIntel')
    expect(shortcutHint('1')).toBe('⌘1')
    expect(shortcutHint(',')).toBe('⌘,')
    setPlatform('Win32')
    expect(shortcutHint('1')).toBe('Ctrl+1')
    expect(shortcutHint(',')).toBe('Ctrl+,')
  })
})

describe('hasOpenOverlay', () => {
  it('无覆盖层时返回 false', () => {
    expect(hasOpenOverlay()).toBe(false)
  })

  it('检测 NModal / NDialog 遮罩与 NPopconfirm 覆盖层', () => {
    const el = document.createElement('div')
    el.className = 'n-modal-mask'
    document.body.appendChild(el)
    expect(hasOpenOverlay()).toBe(true)
    el.remove()
    expect(hasOpenOverlay()).toBe(false)

    const popconfirm = document.createElement('div')
    popconfirm.className = 'n-popconfirm'
    document.body.appendChild(popconfirm)
    expect(hasOpenOverlay()).toBe(true)
    popconfirm.remove()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('弹窗容器的残留空壳（.n-modal-container）不视为打开', () => {
    const shell = document.createElement('div')
    shell.className = 'n-modal-container'
    document.body.appendChild(shell)
    expect(hasOpenOverlay()).toBe(false)
    shell.remove()
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

    const datePanel = document.createElement('div')
    datePanel.className = 'n-date-panel'
    document.body.appendChild(datePanel)
    expect(hasOpenOverlay()).toBe(true)
    datePanel.remove()
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
    expect(push).toHaveBeenCalledWith({ name: 'budget' })
  })

  it('已在目标路由时不重复跳转', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('budget')
    mountHost(router)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
  })

  it('覆盖层打开时抑制跳转', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    const overlay = document.createElement('div')
    overlay.className = 'n-modal-mask'
    document.body.appendChild(overlay)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
    overlay.remove()
  })

  it('弹窗关闭后的残留空壳（.n-modal-container）不再抑制跳转（issue #153 回归：关闭弹窗后快捷键永久失效）', () => {
    // naive-ui VLazyTeleport 关闭后容器永久残留 DOM，存在性嗅探不得以其为信号
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    const shell = document.createElement('div')
    shell.className = 'n-modal-container'
    const hiddenCard = document.createElement('div')
    hiddenCard.className = 'n-card'
    hiddenCard.style.display = 'none'
    shell.appendChild(hiddenCard)
    document.body.appendChild(shell)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).toHaveBeenCalledWith({ name: 'budget' })
    shell.remove()
  })

  it('未命中快捷键时不跳转也不报错', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    window.dispatchEvent(press('4', { ctrlKey: true }))
    window.dispatchEvent(press('x', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
  })
})

describe('启动读路径（issue #269：读取已存顺序，经解析防御后派生键位）', () => {
  afterEach(() => {
    localStorage.removeItem(VIEW_STATE_KEYS.sidebarOrder)
    vi.resetModules()
  })

  it('已存非空顺序：非法项过滤、去重、缺失项按默认序补入末尾，键位随最终位置', async () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify(['reports', 'transactions', 'bogus', 'reports']),
    )
    vi.resetModules()
    const mod = await import('@/composables/useViewShortcuts')
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([
      'dashboard',
      'reports',
      'transactions',
      'accounts',
      'budget',
      'investments',
      'scheduled',
      'items',
      'search',
      'ai',
      'settings',
    ])
    expect(mod.viewShortcuts.value.find((s) => s.name === 'reports')!.key).toBe('2')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'transactions')!.key).toBe('3')
  })

  it('读取为空时回退默认序（本票尚无写路径，读取恒为空）', async () => {
    vi.resetModules()
    const mod = await import('@/composables/useViewShortcuts')
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('5')
  })
})
