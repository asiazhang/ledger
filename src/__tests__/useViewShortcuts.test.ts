import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import type { Router } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import AppDropdown from '@/components/AppDropdown.vue'
import { createOverlayToken, openOverlayNames, resetOverlays, hasOpenOverlay } from '@/composables/overlayRegistry'
import { deriveViewShortcuts, matchViewShortcut, shortcutHint, useViewShortcuts } from '@/composables/useViewShortcuts'
import {
  useSidebarOrderStore,
  DEFAULT_VIEW_ORDER,
  ARRANGEABLE_VIEWS,
} from '@/stores/sidebar-order'
import type { SidebarGroupOrders } from '@/stores/sidebar-order'
import { VIEW_STATE_KEYS } from '@/utils/view-state'

// 视图快捷键（键位带段）测试：键位带推导纯逻辑 + 键盘注册，经 sidebar-order store 装配
// （issue #549：顺序状态归位 store，此处只测键位面对 store 组内序的响应；排序/收纳的
// store 接口测试见 sidebar-order.test.ts）。「重启」惯用法 = setActivePinia(createPinia())。

beforeEach(() => {
  localStorage.clear()
  setActivePinia(createPinia())
})

function setPlatform(platform: string) {
  Object.defineProperty(navigator, 'platform', { value: platform, configurable: true })
}

function press(key: string, mods: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean } = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key, bubbles: true, ...mods })
}

afterEach(() => setPlatform(''))

/** 默认组内序（= SIDEBAR_GROUPS 成员展开）：派生断言复用 */
const DEFAULT_ORDERS: SidebarGroupOrders = {
  bookkeeping: ['transactions', 'accounts', 'budget'],
  assets: ['investments', 'items'],
  insights: ['reports', 'search'],
}

describe('viewShortcuts 派生（键位只扫主项、固定组带，issue #473 / ADR-0065 取代 ADR-0063 决策 2 线性推导）', () => {
  it('出厂键位：概览=`、主项按固定组带占 1..5/7..8、⌘6/⌘9 带内空置（无任何记录占用）、AI=0、设置为逗号键', () => {
    const shortcuts = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)
    expect(shortcuts.map((s) => s.key)).toEqual([
      '`', '1', '2', '3', '4', '5', '7', '8', '0', ',',
    ])
    expect(shortcuts.map((s) => s.name)).toEqual([...DEFAULT_VIEW_ORDER])
  })

  it('每个侧栏主项/固定项恰一条记录；键位随固定组带：交易=1、账户=2、预算=3、投资=4、物品=5、报表=7、搜索=8、AI=0', () => {
    const shortcuts = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)
    const names = shortcuts.map((s) => s.name)
    expect(new Set(names).size).toBe(names.length)
    expect(new Set(names)).toEqual(new Set(DEFAULT_VIEW_ORDER))
    const keyOf = (name: string) => shortcuts.find((v) => v.name === name)!.key
    expect(keyOf('settings')).toBe(',')
    expect(keyOf('dashboard')).toBe('`')
    expect(keyOf('transactions')).toBe('1')
    expect(keyOf('accounts')).toBe('2')
    expect(keyOf('budget')).toBe('3')
    expect(keyOf('investments')).toBe('4')
    expect(keyOf('items')).toBe('5')
    expect(keyOf('reports')).toBe('7')
    expect(keyOf('search')).toBe('8')
    expect(keyOf('ai')).toBe('0')
  })

  it('收纳成员与「更多」不入键位表：无键位、不出提示、不可键盘触发', () => {
    const names = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders).map((s) => s.name)
    for (const contained of ['scheduled', 'merchants', 'policies', 'physicalAssets', 'more']) {
      expect(names, contained).not.toContain(contained)
    }
  })

  it('deriveViewShortcuts：出厂七主项占 ⌘1–⌘5、⌘7、⌘8，⌘6/⌘9 带内空置（组内补足后自然回填）', () => {
    const shortcuts = deriveViewShortcuts(DEFAULT_ORDERS)
    expect(shortcuts.find((s) => s.name === 'transactions')!.key).toBe('1')
    expect(shortcuts.find((s) => s.name === 'search')!.key).toBe('8')
    expect(shortcuts.filter((s) => s.key === '6' || s.key === '9')).toHaveLength(0)
    expect(shortcuts.find((s) => s.name === 'ai')!.key).toBe('0')
  })

  it('主项集恒有键位：任意组内序下七主项全部有键位（组内序定带内位置，不跨组压缩）', () => {
    const reordered = deriveViewShortcuts({ ...DEFAULT_ORDERS, insights: ['search', 'reports'], assets: ['items', 'investments'] })
    for (const name of ARRANGEABLE_VIEWS) {
      expect(reordered.find((s) => s.name === name)!.key, name).not.toBeNull()
    }
    expect(reordered.find((s) => s.name === 'items')!.key).toBe('4')
    expect(reordered.find((s) => s.name === 'investments')!.key).toBe('5')
    expect(reordered.find((s) => s.name === 'search')!.key).toBe('7')
    expect(reordered.find((s) => s.name === 'reports')!.key).toBe('8')
  })

  it('组内重排即带内重排键位（键随组内位置，波及面限本组带内）', () => {
    const reordered: SidebarGroupOrders = { ...DEFAULT_ORDERS, bookkeeping: ['budget', 'transactions', 'accounts'] }
    const shortcuts = deriveViewShortcuts(reordered)
    expect(shortcuts.find((s) => s.name === 'budget')!.key).toBe('1')
    expect(shortcuts.find((s) => s.name === 'accounts')!.key).toBe('3')
  })
})

describe('移入后键位重推导（issue #474 / ADR-0065：主项集变化，仅本组带内键位重排，他组不受牵连）', () => {
  it('交易移入更多后：账户=1、预算=2（带首锚定），他组键位原样（投资=4、物品=5、报表=7、搜索=8），⌘3/⌘6/⌘9 空置，移入成员退出键位表', () => {
    const afterMoveIn = deriveViewShortcuts({
      ...DEFAULT_ORDERS,
      bookkeeping: ['accounts', 'budget'],
    })
    expect(afterMoveIn.find((s) => s.name === 'accounts')!.key).toBe('1')
    expect(afterMoveIn.find((s) => s.name === 'budget')!.key).toBe('2')
    expect(afterMoveIn.find((s) => s.name === 'investments')!.key).toBe('4')
    expect(afterMoveIn.find((s) => s.name === 'items')!.key).toBe('5')
    expect(afterMoveIn.find((s) => s.name === 'reports')!.key).toBe('7')
    expect(afterMoveIn.find((s) => s.name === 'search')!.key).toBe('8')
    expect(afterMoveIn.filter((s) => s.key === '3' || s.key === '6' || s.key === '9')).toHaveLength(0)
    expect(afterMoveIn.find((s) => s.name === 'transactions')).toBeUndefined()
  })

  it('移入空组（洞察）后：报表移入、洞察组主项只剩搜索（带首），他组键位不受牵连', () => {
    const afterMoveIn = deriveViewShortcuts({
      ...DEFAULT_ORDERS,
      insights: ['search'],
    })
    expect(afterMoveIn.find((s) => s.name === 'transactions')!.key).toBe('1')
    expect(afterMoveIn.find((s) => s.name === 'items')!.key).toBe('5')
    expect(afterMoveIn.find((s) => s.name === 'search')!.key).toBe('7')
    expect(afterMoveIn.find((s) => s.name === 'reports')).toBeUndefined()
  })
})

describe('键位随动（store 写路径 → 键位带装配，issue #549：状态在 store，键位面只读响应）', () => {
  it('组内排序后键位带内随动：搜索上移到组首得 7、报表落组末 8；他组键位不受牵连', () => {
    const store = useSidebarOrderStore()
    store.applySidebarSort('search', 'top')
    const keyOf = (name: string) => deriveViewShortcuts(store.sidebarGroupOrders).find((s) => s.name === name)!.key
    // 他组键位原样（固定键位带：记账 1-3、资产 4-6、洞察 7-9）
    expect(keyOf('transactions')).toBe('1')
    expect(keyOf('budget')).toBe('3')
    expect(keyOf('investments')).toBe('4')
    expect(keyOf('items')).toBe('5')
    // 本组内键位随动（主项恒有键位，⌘6/⌘9 出厂空置）
    expect(keyOf('search')).toBe('7')
    expect(keyOf('reports')).toBe('8')
  })

  it('移入更多后键位重排：主项集变化，仅本组带内前移，移入成员退出键位表', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    const keyOf = (name: string) => deriveViewShortcuts(store.sidebarGroupOrders).find((s) => s.name === name)?.key
    expect(keyOf('accounts')).toBe('1')
    expect(keyOf('investments')).toBe('4')
    expect(keyOf('transactions')).toBeUndefined()
  })

  it('移回侧栏后键位重排：移回成员落本组带内末位，他组不受牵连', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    store.applyMoveBackToSidebar('scheduled')
    const keyOf = (name: string) => deriveViewShortcuts(store.sidebarGroupOrders).find((s) => s.name === name)!.key
    expect(keyOf('accounts')).toBe('1')
    expect(keyOf('budget')).toBe('2')
    expect(keyOf('scheduled')).toBe('3')
    expect(keyOf('investments')).toBe('4')
  })

  it('重启（新 pinia）后键位随持久化组内序一致', () => {
    useSidebarOrderStore().applySidebarSort('search', 'top')
    setActivePinia(createPinia())
    const shortcuts = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)
    expect(shortcuts.map((s) => s.name)).toEqual([
      'dashboard', 'transactions', 'accounts', 'budget', 'investments', 'items', 'search', 'reports', 'ai', 'settings',
    ])
    expect(shortcuts.find((s) => s.name === 'search')!.key).toBe('7')
    expect(shortcuts.find((s) => s.name === 'reports')!.key).toBe('8')
  })
})

describe('启动读路径（issue #269/#359：读取已存组内序，经解析防御后派生键位；store 装配）', () => {
  it('已存组内序对象：各组独立解析生效，键位随组内带内位置', () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify({ bookkeeping: ['budget', 'transactions'], insights: ['search', 'reports'] }),
    )
    setActivePinia(createPinia())
    const shortcuts = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)
    expect(shortcuts.map((s) => s.name)).toEqual([
      'dashboard',
      'budget',
      'transactions',
      'accounts',
      'investments',
      'items',
      'search',
      'reports',
      'ai',
      'settings',
    ])
    const keyOf = (name: string) => shortcuts.find((s) => s.name === name)!.key
    expect(keyOf('budget')).toBe('1')
    expect(keyOf('transactions')).toBe('2')
    expect(keyOf('search')).toBe('7')
    expect(keyOf('reports')).toBe('8')
    expect(keyOf('ai')).toBe('0')
  })

  it('已存旧平铺排序数据（issue #270 形态）启动不异常、回退默认序', () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify(['search', 'items', 'scheduled', 'reports', 'investments', 'budget', 'accounts', 'transactions']),
    )
    setActivePinia(createPinia())
    const shortcuts = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)
    expect(shortcuts.map((s) => s.name)).toEqual([...DEFAULT_VIEW_ORDER])
  })

  it('读取为空时回退默认序（未自定义或恢复默认后存储为空）', () => {
    const shortcuts = deriveViewShortcuts(useSidebarOrderStore().sidebarGroupOrders)
    expect(shortcuts.map((s) => s.name)).toEqual([...DEFAULT_VIEW_ORDER])
    expect(shortcuts.find((s) => s.name === 'investments')!.key).toBe('4')
  })
})

describe('matchViewShortcut', () => {
  it('macOS 上 Cmd+` 与 Cmd+数字按固定组带命中对应视图', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('`', { metaKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('1', { metaKey: true }))).toBe('transactions')
    expect(matchViewShortcut(press('2', { metaKey: true }))).toBe('accounts')
    expect(matchViewShortcut(press('3', { metaKey: true }))).toBe('budget')
    expect(matchViewShortcut(press('4', { metaKey: true }))).toBe('investments')
    expect(matchViewShortcut(press('5', { metaKey: true }))).toBe('items')
    expect(matchViewShortcut(press('7', { metaKey: true }))).toBe('reports')
    expect(matchViewShortcut(press('8', { metaKey: true }))).toBe('search')
    expect(matchViewShortcut(press('0', { metaKey: true }))).toBe('ai')
    expect(matchViewShortcut(press(',', { metaKey: true }))).toBe('settings')
  })

  it('⌘6/⌘9 带内空置不命中任何视图；收纳成员与「更多」不可经键盘触发（组内任意重排亦然）', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('6', { metaKey: true }))).toBeNull()
    expect(matchViewShortcut(press('9', { metaKey: true }))).toBeNull()
    for (const k of ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0']) {
      expect(matchViewShortcut(press(k, { metaKey: true }))).not.toBe('scheduled')
      expect(matchViewShortcut(press(k, { metaKey: true }))).not.toBe('merchants')
      expect(matchViewShortcut(press(k, { metaKey: true }))).not.toBe('more')
    }
  })

  it('macOS 上 Ctrl+数字不命中（需要 Cmd）', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBeNull()
  })

  it('非 macOS 上 Ctrl+` 与 Ctrl+数字命中对应视图', () => {
    setPlatform('Win32')
    expect(matchViewShortcut(press('`', { ctrlKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBe('transactions')
    expect(matchViewShortcut(press('3', { ctrlKey: true }))).toBe('budget')
    expect(matchViewShortcut(press('5', { ctrlKey: true }))).toBe('items')
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

  it('命中随 store 组内序实时装配：组内排序后同键命中新视图（键随组内位置）', () => {
    setPlatform('MacIntel')
    useSidebarOrderStore().applySidebarSort('budget', 'top')
    expect(matchViewShortcut(press('1', { metaKey: true }))).toBe('budget')
    expect(matchViewShortcut(press('2', { metaKey: true }))).toBe('transactions')
  })
})

describe('shortcutHint', () => {
  it('macOS 显示 ⌘N，其余显示 ⌃N（Control 键符，与 ⌘ 同族等宽）；逗号键同样适用', () => {
    setPlatform('MacIntel')
    expect(shortcutHint('1')).toBe('⌘1')
    expect(shortcutHint(',')).toBe('⌘,')
    setPlatform('Win32')
    expect(shortcutHint('1')).toBe('⌃1')
    expect(shortcutHint(',')).toBe('⌃,')
  })
})

describe('hasOpenOverlay（弹层注册表，ADR-0035）', () => {
  afterEach(() => resetOverlays())

  it('无弹层上报时返回 false', () => {
    expect(hasOpenOverlay()).toBe(false)
  })

  it.each(['modal', 'popconfirm', 'dropdown', 'select', 'date-picker', 'tree-select', 'dialog'])(
    '弹层 %s 上报打开后返回 true，撤销后恢复 false',
    (name) => {
      const token = createOverlayToken(name)
      expect(hasOpenOverlay()).toBe(false)
      token.set(true)
      expect(hasOpenOverlay()).toBe(true)
      token.set(false)
      expect(hasOpenOverlay()).toBe(false)
    },
  )

  it('重复上报同值不叠加（幂等）', () => {
    const token = createOverlayToken('select')
    token.set(true)
    token.set(true)
    expect(openOverlayNames()).toEqual(['select'])
    token.set(false)
    token.set(false)
    expect(hasOpenOverlay()).toBe(false)
  })

  it('同类弹层多实例各自持有 token：一个关闭不影响另一个', () => {
    const a = createOverlayToken('select')
    const b = createOverlayToken('select')
    a.set(true)
    b.set(true)
    expect(hasOpenOverlay()).toBe(true)
    a.set(false)
    expect(hasOpenOverlay()).toBe(true)
    b.set(false)
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

describe('useViewShortcuts（keydown 注册）', () => {
  it('命中快捷键时跳转到目标路由', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    window.dispatchEvent(press('3', { metaKey: true }))
    expect(push).toHaveBeenCalledWith({ name: 'budget' })
  })

  it('已在目标路由时不重复跳转', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('budget')
    mountHost(router)
    window.dispatchEvent(press('3', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
  })

  it('弹层上报打开时抑制跳转（注册表信号，ADR-0035）', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    const token = createOverlayToken('modal')
    token.set(true)
    window.dispatchEvent(press('3', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
    token.set(false)
    window.dispatchEvent(press('3', { metaKey: true }))
    expect(push).toHaveBeenCalledWith({ name: 'budget' })
    resetOverlays()
  })

  it('未命中快捷键时不跳转也不报错', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    window.dispatchEvent(press('4', { ctrlKey: true }))
    window.dispatchEvent(press('x', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
  })

  it('写路径改组内序后，注册的键位随动命中新视图（装配同一 store）', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    useSidebarOrderStore().applySidebarSort('budget', 'top')
    window.dispatchEvent(press('1', { metaKey: true }))
    expect(push).toHaveBeenCalledWith({ name: 'budget' })
  })
})

describe('排序菜单打开期间视图快捷键被弹层抑制机制压制（issue #270，零新代码）', () => {
  afterEach(() => resetOverlays())

  it('真实 AppDropdown（侧栏排序菜单同款）打开即上报弹层注册表', async () => {
    const Host = defineComponent({
      setup() {
        return () =>
          h(AppDropdown, { trigger: 'manual', show: true, placement: 'bottom-start', options: [{ label: '上移一位', key: 'up' }] }, { default: () => h('div') })
      },
    })
    const w = mount(Host, { attachTo: document.body })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)
    w.unmount()
  })

  it('弹层上报打开期间 Cmd+数字不跳转（含将来换弹层形态的契约）', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    createOverlayToken('dropdown').set(true)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
    resetOverlays()
  })
})
