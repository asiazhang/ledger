import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import type { Router } from 'vue-router'
import AppDropdown from '@/components/AppDropdown.vue'
import { createOverlayToken, openOverlayNames, resetOverlays } from '@/composables/overlayRegistry'
import {
  SIDEBAR_GROUPS,
  DEFAULT_VIEW_ORDER,
  ARRANGEABLE_VIEWS,
  FIRST_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
  viewShortcuts,
  deriveViewShortcuts,
  parseGroupOrders,
  matchViewShortcut,
  shortcutHint,
  hasOpenOverlay,
  useViewShortcuts,
  isArrangeableView,
  isSidebarSortAction,
  groupOfView,
  moveArrangeable,
  buildSidebarSortMenuOptions,
  applySidebarSort,
  resetSidebarOrder,
  moveIntoContainment,
  applyMoveIntoMore,
  moveBackToSidebar,
  applyMoveBackToSidebar,
  isGroupFull,
  GROUP_MAIN_LIMIT,
  isSidebarMember,
  isViewContained,
  buildTabContextMenuOptions,
  sidebarGroupOrders,
  GROUP_CONTAINMENT_SEEDS,
  parseContainmentLists,
  sidebarContainment,
} from '@/composables/useViewShortcuts'
import type { ViewName, ContainableViewName, SidebarGroupOrders, SidebarContainmentLists } from '@/composables/useViewShortcuts'
import type { DropdownOption } from 'naive-ui'
import { VIEW_STATE_KEYS } from '@/utils/view-state'

function setPlatform(platform: string) {
  Object.defineProperty(navigator, 'platform', { value: platform, configurable: true })
}

function press(key: string, mods: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean } = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key, bubbles: true, ...mods })
}

afterEach(() => setPlatform(''))

describe('侧栏分组常量（issue #359 / ADR-0051；#473 记账组主项三项化，ADR-0063 决策 2/3）', () => {
  it('三组锁定：记账 = 交易、账户、预算；资产 = 投资、物品；洞察 = 报表、搜索（定时/商户为收纳成员）', () => {
    expect(SIDEBAR_GROUPS.map((g) => g.id)).toEqual(['bookkeeping', 'assets', 'insights'])
    expect(SIDEBAR_GROUPS.map((g) => [...g.views])).toEqual([
      ['transactions', 'accounts', 'budget'],
      ['investments', 'items'],
      ['reports', 'search'],
    ])
  })

  it('线性默认序：概览 + 各组按组序展开 + AI + 设置（分组标题不占位、不计数；全局「更多」已退役）', () => {
    expect([...DEFAULT_VIEW_ORDER]).toEqual([
      'dashboard',
      'transactions',
      'accounts',
      'budget',
      'investments',
      'items',
      'reports',
      'search',
      'ai',
      'settings',
    ])
  })

  it('三固定约束：概览首位、AI 倒数第二、设置末位（全局「更多」固定项 #473 退役，ADR-0063 决策 1）', () => {
    expect(FIRST_VIEW).toBe('dashboard')
    expect(PENULTIMATE_VIEW).toBe('ai')
    expect(LAST_VIEW).toBe('settings')
    expect(DEFAULT_VIEW_ORDER[0]).toBe(FIRST_VIEW)
    expect(DEFAULT_VIEW_ORDER[DEFAULT_VIEW_ORDER.length - 2]).toBe(PENULTIMATE_VIEW)
    expect(DEFAULT_VIEW_ORDER[DEFAULT_VIEW_ORDER.length - 1]).toBe(LAST_VIEW)
  })

  it('主项（可排区）= 各组成员按组序展开共 7 项（定时/商户/保单/实物资产均为收纳成员，不在其中）', () => {
    expect(ARRANGEABLE_VIEWS).toEqual([
      'transactions',
      'accounts',
      'budget',
      'investments',
      'items',
      'reports',
      'search',
    ])
  })

  it('groupOfView：主项各有其组；概览/AI/设置与未知名不在任何组（issue #475：出厂种子词法归属出厂组，移回后即本组主项）', () => {
    expect(groupOfView('transactions')).toBe('bookkeeping')
    expect(groupOfView('investments')).toBe('assets')
    expect(groupOfView('items')).toBe('assets')
    expect(groupOfView('reports')).toBe('insights')
    expect(groupOfView('search')).toBe('insights')
    expect(groupOfView('dashboard')).toBeNull()
    expect(groupOfView('ai')).toBeNull()
    expect(groupOfView('settings')).toBeNull()
    // 出厂收纳种子不跨组（ADR-0063 决策 4）：词法归属出厂组，移回侧栏后即以该组主项身份在册
    expect(groupOfView('scheduled')).toBe('bookkeeping')
    expect(groupOfView('merchants')).toBe('bookkeeping')
    expect(groupOfView('policies')).toBe('assets')
    expect(groupOfView('physicalAssets')).toBe('assets')
    expect(groupOfView('bogus' as ViewName)).toBeNull()
  })
})

/** 默认组内序（= SIDEBAR_GROUPS 成员展开）：解析/持久化断言复用 */
const DEFAULT_ORDERS: SidebarGroupOrders = {
  bookkeeping: ['transactions', 'accounts', 'budget'],
  assets: ['investments', 'items'],
  insights: ['reports', 'search'],
}

describe('parseGroupOrders（组内序解析纯函数）', () => {
  const defaults = DEFAULT_ORDERS

  it('空值 / 非对象 / 数组整体回退默认序', () => {
    for (const junk of [null, undefined, 'junk', 123, true, [], ['search', 'reports']]) {
      expect(parseGroupOrders(junk)).toEqual(defaults)
    }
  })

  it('已存旧平铺排序数据（issue #270 形态的数组）整体回退默认序，不抛异常', () => {
    const legacy = ['transactions', 'accounts', 'budget', 'investments', 'reports', 'items', 'search']
    expect(parseGroupOrders(legacy)).toEqual(defaults)
  })

  it('组内已存顺序保留；非法名（固定项名、他组成员、未知名、非字符串）被过滤，缺失项按默认序补入组内末尾', () => {
    expect(parseGroupOrders({
      bookkeeping: ['budget', 'transactions', 'dashboard', 'investments', 'bogus', 42, 'accounts', 'ai'],
      assets: ['items'],
      insights: 'junk',
    })).toEqual({
      bookkeeping: ['budget', 'transactions', 'accounts'],
      assets: ['items', 'investments'],
      insights: ['reports', 'search'],
    })
  })

  it('存量组内序含定时（#473 迁出前的合法主项）：按非法名过滤回退，不报错、不产生空缺', () => {
    expect(parseGroupOrders({
      bookkeeping: ['transactions', 'accounts', 'budget', 'scheduled'],
      assets: ['investments', 'items'],
      insights: ['reports', 'search'],
    })).toEqual(DEFAULT_ORDERS)
  })

  it('组内重复项去重保留首现', () => {
    expect(parseGroupOrders({ insights: ['search', 'reports', 'search'] })).toEqual({
      ...defaults,
      insights: ['search', 'reports'],
    })
  })

  it('完整组内排列原样返回；组外成员被过滤、其余组保持默认序', () => {
    const full = ['search', 'items', 'scheduled', 'reports', 'investments', 'budget', 'accounts', 'transactions']
    expect(parseGroupOrders({ bookkeeping: full, insights: full })).toEqual({
      bookkeeping: ['budget', 'accounts', 'transactions'],
      assets: defaults.assets,
      insights: ['search', 'reports'],
    })
  })
})

describe('parseGroupOrders × 收纳清单耦合（issue #474：收纳成员不回填主项，清单是成员资格唯一事实源）', () => {
  const contained: SidebarContainmentLists = {
    bookkeeping: ['scheduled', 'merchants', 'transactions'],
    assets: ['policies', 'physicalAssets'],
    insights: [],
  }

  it('第二参数（已解析收纳清单）中的成员不回填组内序：缺失项补尾跳过收纳成员', () => {
    expect(parseGroupOrders({ bookkeeping: ['accounts'] }, contained)).toEqual({
      bookkeeping: ['accounts', 'budget'],
      assets: ['investments', 'items'],
      insights: ['reports', 'search'],
    })
  })

  it('已存数组中的收纳成员按非法名过滤（不因「缺失补尾」复活为主项）', () => {
    expect(
      parseGroupOrders(
        { bookkeeping: ['transactions', 'accounts', 'budget'] },
        contained,
      ).bookkeeping,
    ).toEqual(['accounts', 'budget'])
  })

  it('不带第二参数时行为不变（默认排除出厂种子，种子本就不在主项词表）', () => {
    expect(parseGroupOrders(null)).toEqual(DEFAULT_ORDERS)
  })
})

describe('viewShortcuts 派生（键位只扫主项、固定组带，issue #473 / ADR-0065 取代 ADR-0063 决策 2 线性推导）', () => {
  it('出厂键位：概览=`、主项按固定组带占 1..5/7..8、⌘6/⌘9 带内空置（无任何记录占用）、AI=0、设置为逗号键', () => {
    expect(viewShortcuts.value.map((s) => s.key)).toEqual([
      '`', '1', '2', '3', '4', '5', '7', '8', '0', ',',
    ])
    expect(viewShortcuts.value.map((s) => s.name)).toEqual([...DEFAULT_VIEW_ORDER])
  })

  it('每个侧栏主项/固定项恰一条记录；键位随固定组带：交易=1、账户=2、预算=3、投资=4、物品=5、报表=7、搜索=8、AI=0', () => {
    const names = viewShortcuts.value.map((s) => s.name)
    expect(new Set(names).size).toBe(names.length)
    expect(new Set(names)).toEqual(new Set(DEFAULT_VIEW_ORDER))
    expect(viewShortcuts.value.find((v) => v.name === 'settings')!.key).toBe(',')
    expect(viewShortcuts.value.find((v) => v.name === 'dashboard')!.key).toBe('`')
    expect(viewShortcuts.value.find((v) => v.name === 'transactions')!.key).toBe('1')
    expect(viewShortcuts.value.find((v) => v.name === 'accounts')!.key).toBe('2')
    expect(viewShortcuts.value.find((v) => v.name === 'budget')!.key).toBe('3')
    expect(viewShortcuts.value.find((v) => v.name === 'investments')!.key).toBe('4')
    expect(viewShortcuts.value.find((v) => v.name === 'items')!.key).toBe('5')
    expect(viewShortcuts.value.find((v) => v.name === 'reports')!.key).toBe('7')
    expect(viewShortcuts.value.find((v) => v.name === 'search')!.key).toBe('8')
    expect(viewShortcuts.value.find((v) => v.name === 'ai')!.key).toBe('0')
  })

  it('收纳成员与「更多」不入键位表：无键位、不出提示、不可键盘触发', () => {
    const names = viewShortcuts.value.map((s) => s.name)
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

describe('useViewShortcuts', () => {
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
})

describe('启动读路径（issue #269/#359：读取已存组内序，经解析防御后派生键位）', () => {
  afterEach(() => {
    localStorage.removeItem(VIEW_STATE_KEYS.sidebarOrder)
    vi.resetModules()
  })

  it('已存组内序对象：各组独立解析生效，键位随组内带内位置', async () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify({ bookkeeping: ['budget', 'transactions'], insights: ['search', 'reports'] }),
    )
    vi.resetModules()
    const mod = await import('@/composables/useViewShortcuts')
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([
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
    expect(mod.viewShortcuts.value.find((s) => s.name === 'budget')!.key).toBe('1')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'transactions')!.key).toBe('2')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('7')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'reports')!.key).toBe('8')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'ai')!.key).toBe('0')
  })

  it('已存旧平铺排序数据（issue #270 形态）启动不异常、回退默认序', async () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify(['search', 'items', 'scheduled', 'reports', 'investments', 'budget', 'accounts', 'transactions']),
    )
    vi.resetModules()
    const mod = await import('@/composables/useViewShortcuts')
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
    expect(mod.sidebarGroupOrders.value).toEqual(mod.parseGroupOrders(null))
  })

  it('读取为空时回退默认序（未自定义或恢复默认后存储为空）', async () => {
    vi.resetModules()
    const mod = await import('@/composables/useViewShortcuts')
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('4')
  })
})

describe('组内收纳清单：出厂种子与解析防御（issue #472/#473 / ADR-0063 决策 3/5）', () => {
  it('出厂种子锁定：记账 = [定时, 商户]（#473）；资产 = [保单, 实物资产]；洞察 = 空（出厂不渲染链接）', () => {
    expect(GROUP_CONTAINMENT_SEEDS.bookkeeping).toEqual(['scheduled', 'merchants'])
    expect(GROUP_CONTAINMENT_SEEDS.assets).toEqual(['policies', 'physicalAssets'])
    expect(GROUP_CONTAINMENT_SEEDS.insights).toEqual([])
  })

  it('非对象整体回出厂种子（null/undefined/数组/标量）', () => {
    const seeds = { bookkeeping: ['scheduled', 'merchants'], assets: ['policies', 'physicalAssets'], insights: [] }
    expect(parseContainmentLists(null)).toEqual(seeds)
    expect(parseContainmentLists(undefined)).toEqual(seeds)
    expect(parseContainmentLists(['policies'])).toEqual(seeds)
    expect(parseContainmentLists('x')).toEqual(seeds)
    expect(parseContainmentLists(42)).toEqual(seeds)
  })

  it('各组独立解析：组值非数组该组回种子，他组不受牵连', () => {
    expect(parseContainmentLists({ bookkeeping: [], assets: 'x', insights: [] })).toEqual({
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets'],
      insights: [],
    })
  })

  it('非法名过滤：他组主项/他组种子/固定项名/未知名/非字符串一律不入清单（不跨组）', () => {
    const raw = {
      assets: ['transactions', 'scheduled', 'more', 'dashboard', 'settings', 42, 'bogus', 'policies'],
      bookkeeping: ['policies'],
    }
    const parsed = parseContainmentLists(raw)
    expect(parsed.assets).toEqual(['policies', 'physicalAssets']) // 合法名只剩保单，实物资产作缺失出厂成员补尾
    expect(parsed.bookkeeping).toEqual(['scheduled', 'merchants']) // 保单是他组成员，清单回出厂种子
  })

  it('本组主项名合法（issue #474 用户移入）：清单序保留，缺失种子仍按出厂序补尾', () => {
    const parsed = parseContainmentLists({
      bookkeeping: ['scheduled', 'transactions', 'merchants'],
      insights: ['search'],
    })
    expect(parsed.bookkeeping).toEqual(['scheduled', 'transactions', 'merchants'])
    expect(parsed.insights).toEqual(['search'])
  })

  it('去重保留首现；缺失出厂成员按出厂序补尾（自定义成员保位，缺失者缀后）', () => {
    expect(parseContainmentLists({ bookkeeping: ['merchants', 'merchants'] }).bookkeeping).toEqual(['merchants', 'scheduled'])
    expect(parseContainmentLists({ assets: ['policies', 'policies'] }).assets).toEqual(['policies', 'physicalAssets'])
    expect(parseContainmentLists({ bookkeeping: [] }).bookkeeping).toEqual(['scheduled', 'merchants'])
    expect(parseContainmentLists({ insights: [] }).insights).toEqual([])
  })
})

describe('收纳清单启动读路径与复位（issue #472/#473 / ADR-0063：ViewState 持久化，恢复默认连收纳一起复位）', () => {
  const KEY = VIEW_STATE_KEYS.sidebarContainment

  beforeEach(() => {
    localStorage.clear()
    vi.resetModules()
  })

  afterEach(() => {
    localStorage.removeItem(KEY)
    localStorage.removeItem(VIEW_STATE_KEYS.sidebarOrder)
    vi.resetModules()
  })

  async function fresh() {
    return await import('@/composables/useViewShortcuts')
  }

  it('读取为空时回出厂种子（未自定义或恢复默认后存储为空）', async () => {
    const mod = await fresh()
    expect(mod.sidebarContainment.value).toEqual({
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets'],
      insights: [],
    })
  })

  it('已存收纳清单对象：各组独立经解析防御生效（脏清单回种子，跨启动不异常）', async () => {
    localStorage.setItem(KEY, JSON.stringify({ assets: ['merchants'], insights: ['x'] }))
    vi.resetModules()
    const mod = await fresh()
    expect(mod.sidebarContainment.value).toEqual({
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets'],
      insights: [],
    })
  })

  it('存储往返：saveContainmentLists 写入的清单经 parseContainmentLists 防御后还原', async () => {
    const mod = await fresh()
    const vs = await import('@/utils/view-state')
    const lists = { bookkeeping: ['scheduled', 'merchants'], assets: ['policies', 'physicalAssets'], insights: [] }
    vs.saveContainmentLists(lists)
    expect(JSON.parse(localStorage.getItem(KEY)!)).toEqual(lists)
    expect(mod.parseContainmentLists(JSON.parse(localStorage.getItem(KEY)!))).toEqual(lists)
  })

  it('resetSidebarOrder 同时复位组内序与收纳清单：两个存储键清除、状态回出厂', async () => {
    const mod = await fresh()
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify({ bookkeeping: ['budget'], assets: ['items'], insights: ['reports'] }),
    )
    localStorage.setItem(KEY, JSON.stringify({ assets: ['policies'] })) // 脏清单：缺实物资产，解析补尾
    vi.resetModules()
    const rebooted = await fresh()
    // 自定义组内序在（非出厂）
    expect(rebooted.sidebarGroupOrders.value).not.toEqual(rebooted.parseGroupOrders(null))
    rebooted.resetSidebarOrder()
    expect(localStorage.getItem(VIEW_STATE_KEYS.sidebarOrder)).toBeNull()
    expect(localStorage.getItem(KEY)).toBeNull()
    expect(rebooted.sidebarGroupOrders.value).toEqual(rebooted.parseGroupOrders(null))
    expect(rebooted.sidebarContainment.value).toEqual({
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets'],
      insights: [],
    })
  })
})

/** 菜单选项测试取值助手：把 DropdownOption 收窄到本菜单用到的字段 */
function row(o: DropdownOption) {
  return o as { label?: string; key?: string; disabled?: boolean; type?: string }
}

describe('moveArrangeable（组内移动纯函数，issue #270/#359）', () => {
  const bookkeeping: ContainableViewName[] = ['transactions', 'accounts', 'budget']

  it('组内上移一位：与前一项交换，其余相对顺序不变', () => {
    expect(moveArrangeable(bookkeeping, 'accounts', 'up')).toEqual([
      'accounts', 'transactions', 'budget',
    ])
  })

  it('组内下移一位：与后一项交换，其余相对顺序不变', () => {
    expect(moveArrangeable(bookkeeping, 'accounts', 'down')).toEqual([
      'transactions', 'budget', 'accounts',
    ])
  })

  it('移到组内顶部', () => {
    expect(moveArrangeable(bookkeeping, 'budget', 'top')).toEqual([
      'budget', 'transactions', 'accounts',
    ])
  })

  it('移到组内底部', () => {
    expect(moveArrangeable(bookkeeping, 'accounts', 'bottom')).toEqual([
      'transactions', 'budget', 'accounts',
    ])
  })

  it('组内边界 no-op：首位上移/移顶、末位下移/移底内容不变，且返回新数组不改输入', () => {
    expect(moveArrangeable(bookkeeping, 'transactions', 'up')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'transactions', 'top')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'budget', 'down')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'budget', 'bottom')).toEqual(bookkeeping)
    const upResult = moveArrangeable(bookkeeping, 'transactions', 'up')
    expect(upResult).not.toBe(bookkeeping)
    expect(bookkeeping).toEqual(['transactions', 'accounts', 'budget'])
  })

  it('在自定义序上同样正确：已置顶项再上移为 no-op', () => {
    const custom = moveArrangeable(bookkeeping, 'budget', 'top')
    expect(moveArrangeable(custom, 'budget', 'up')).toEqual(custom)
  })

  it('目标项不在序中（如固定项名、他组成员、收纳成员）：原样返回内容——移动只见一张组内数组，跨组移动结构上不可达', () => {
    expect(moveArrangeable(bookkeeping, 'ai', 'up')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'search', 'bottom')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'scheduled' as ViewName, 'bottom')).toEqual(bookkeeping)
  })
})

describe('buildSidebarSortMenuOptions（排序菜单选项构建纯函数，含组内边界置灰，issue #270/#359）', () => {
  const bookkeeping: ContainableViewName[] = ['transactions', 'accounts', 'budget']

  it('菜单形状：四种移动 + 分隔线 + 移入更多 + 分隔线 + 恢复默认排序（issue #474：移入在排序动作后、分隔线隔开）', () => {
    const opts = buildSidebarSortMenuOptions('accounts', bookkeeping)
    expect(opts.map(row).map((o) => o.label ?? o.type)).toEqual([
      '上移一位', '下移一位', '移到顶部', '移到底部', 'divider', '移入更多', 'divider', '恢复默认排序',
    ])
    expect(opts.map(row).map((o) => o.key)).toEqual([
      'up', 'down', 'top', 'bottom', 'sort-divider', 'intoMore', 'reset-divider', 'reset',
    ])
  })

  it('菜单 key 与移动动作同一词表（防两套词表错位回归：菜单里的移动 key 必须全是合法 SidebarSortAction）', () => {
    const moveKeys = buildSidebarSortMenuOptions('budget', bookkeeping)
      .map(row)
      .map((o) => o.key!)
      .filter((k) => k !== 'sort-divider' && k !== 'reset-divider' && k !== 'reset' && k !== 'intoMore')
    expect(moveKeys).toEqual(['up', 'down', 'top', 'bottom'])
    for (const k of moveKeys) expect(isSidebarSortAction(k)).toBe(true)
    // 每个菜单 key 直接可作为动作施加，效果与菜单语义一致（key 即 action）
    expect(moveArrangeable(bookkeeping, 'accounts', 'up')[0]).toBe('accounts')
    expect(moveArrangeable(bookkeeping, 'budget', 'bottom').at(-1)).toBe('budget')
  })

  it('组内首位项：上移/移顶置灰，下移/移底可用', () => {
    const [up, down, top, bottom] = buildSidebarSortMenuOptions('transactions', bookkeeping).map(row)
    expect(up!.disabled).toBe(true)
    expect(top!.disabled).toBe(true)
    expect(down!.disabled).toBe(false)
    expect(bottom!.disabled).toBe(false)
  })

  it('组内末位项：下移/移底置灰，上移/移顶可用', () => {
    const [up, down, top, bottom] = buildSidebarSortMenuOptions('budget', bookkeeping).map(row)
    expect(down!.disabled).toBe(true)
    expect(bottom!.disabled).toBe(true)
    expect(up!.disabled).toBe(false)
    expect(top!.disabled).toBe(false)
  })

  it('组内中间项：四种移动全部可用；移入更多与恢复默认排序恒可用（issue #474 移入自由）', () => {
    const opts = buildSidebarSortMenuOptions('accounts', bookkeeping).map(row)
    for (const o of opts) {
      if (o.type === 'divider') continue
      expect(o.disabled).toBe(false)
    }
    expect(opts[5]!.key).toBe('intoMore')
    expect(opts[7]!.key).toBe('reset')
  })

  it('自定义序下的边界按当前组内序判定', () => {
    const custom = moveArrangeable(bookkeeping, 'budget', 'top')
    const [up, , top] = buildSidebarSortMenuOptions('budget', custom).map(row)
    expect(up!.disabled).toBe(true)
    expect(top!.disabled).toBe(true)
  })
})

describe('isArrangeableView（固定项例外判定，issue #270/#359；#473 主项七项）', () => {
  it('主项（可排区）为真；概览/AI/设置三固定项与收纳成员为假', () => {
    for (const name of ARRANGEABLE_VIEWS) expect(isArrangeableView(name)).toBe(true)
    expect(isArrangeableView('dashboard')).toBe(false)
    expect(isArrangeableView('ai')).toBe(false)
    expect(isArrangeableView('settings')).toBe(false)
    expect(isArrangeableView('scheduled')).toBe(false)
    expect(isArrangeableView('policies')).toBe(false)
    expect(isArrangeableView('bogus')).toBe(false)
  })
})

describe('写路径与持久化（issue #270/#359：组内点选即重排、立即持久化、重启保持、恢复默认）', () => {
  const ORDER_KEY = VIEW_STATE_KEYS.sidebarOrder

  beforeEach(() => {
    localStorage.clear()
    vi.resetModules()
  })

  afterEach(() => {
    localStorage.removeItem(ORDER_KEY)
    vi.resetModules()
  })

  async function fresh() {
    return await import('@/composables/useViewShortcuts')
  }

  it('applySidebarSort：组内响应式重排 + 立即持久化（对象形状「组 id → 视图名数组」）', async () => {
    const mod = await fresh()
    mod.applySidebarSort('search', 'top')
    expect(mod.sidebarGroupOrders.value.insights[0]).toBe('search')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('7')
    const stored = JSON.parse(localStorage.getItem(ORDER_KEY)!)
    expect(stored).toEqual({
      bookkeeping: ['transactions', 'accounts', 'budget'],
      assets: ['investments', 'items'],
      insights: ['search', 'reports'],
    })
  })

  it('组内排序不越组：他组数组与他组键位不受牵连', async () => {
    const mod = await fresh()
    mod.applySidebarSort('search', 'top')
    // 他组顺序原样
    expect(mod.sidebarGroupOrders.value.bookkeeping).toEqual(['transactions', 'accounts', 'budget'])
    expect(mod.sidebarGroupOrders.value.assets).toEqual(['investments', 'items'])
    // 他组键位原样（固定键位带：记账 1-3、资产 4-6、洞察 7-9）
    expect(mod.viewShortcuts.value.find((s) => s.name === 'transactions')!.key).toBe('1')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'budget')!.key).toBe('3')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('4')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'items')!.key).toBe('5')
    // 本组内键位随动：搜索上移到组首得 7，报表落到组末 8（主项恒有键位，⌘6/⌘9 出厂空置）
    expect(mod.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('7')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'reports')!.key).toBe('8')
  })

  it('固定项不参与排序（applySidebarSort 对固定项为 no-op，不写存储）', async () => {
    const mod = await fresh()
    mod.applySidebarSort('dashboard', 'bottom')
    mod.applySidebarSort('ai', 'top')
    mod.applySidebarSort('settings', 'up')
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
  })

  it('重启（重导入）后组内自定义序保持，键位随新位置一致', async () => {
    const mod = await fresh()
    mod.applySidebarSort('search', 'top')
    vi.resetModules()
    const rebooted = await fresh()
    expect(rebooted.viewShortcuts.value.map((s) => s.name)).toEqual([
      'dashboard', 'transactions', 'accounts', 'budget', 'investments', 'items', 'search', 'reports', 'ai', 'settings',
    ])
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('7')
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'reports')!.key).toBe('8')
  })

  it('save→load→解析往返：持久化值经 parseGroupOrders 防御后还原', async () => {
    const mod = await fresh()
    mod.applySidebarSort('items', 'top')
    mod.applySidebarSort('search', 'up')
    // 直接读存储做解析往返（等价启动读路径）
    const raw = JSON.parse(localStorage.getItem(ORDER_KEY)!)
    expect(mod.parseGroupOrders(raw)).toEqual({ ...mod.sidebarGroupOrders.value })
  })

  it('resetSidebarOrder：清除存储回默认序，可反复「自定义 → 恢复 → 再自定义」交替', async () => {
    const mod = await fresh()
    mod.applySidebarSort('items', 'top')
    expect(localStorage.getItem(ORDER_KEY)).not.toBeNull()
    mod.resetSidebarOrder()
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
    // 恢复后可再次自定义
    mod.applySidebarSort('accounts', 'bottom')
    expect(localStorage.getItem(ORDER_KEY)).not.toBeNull()
    expect(mod.sidebarGroupOrders.value.bookkeeping[mod.sidebarGroupOrders.value.bookkeeping.length - 1]).toBe('accounts')
    // 再恢复，仍回默认
    mod.resetSidebarOrder()
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
  })

  it('边界移动（组内首位上移）不改顺序且不写存储：保住「恢复默认 = 删 key」语义，出厂序调整时自动跟随', async () => {
    const mod = await fresh()
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    mod.applySidebarSort('transactions', 'up')
    expect(mod.sidebarGroupOrders.value).toEqual(mod.parseGroupOrders(null))
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
  })
})

describe('moveIntoContainment（移入纯函数，issue #474 / ADR-0063 决策 4：移入自由）', () => {
  const lists: SidebarContainmentLists = {
    bookkeeping: ['scheduled', 'merchants'],
    assets: ['policies', 'physicalAssets'],
    insights: [],
  }

  it('追加为本组收纳清单尾（清单序 = 页签序）：他组不受牵连、输入不改', () => {
    const next = moveIntoContainment(lists, 'bookkeeping', 'transactions')
    expect(next.bookkeeping).toEqual(['scheduled', 'merchants', 'transactions'])
    expect(next.assets).toEqual(['policies', 'physicalAssets'])
    expect(next.insights).toEqual([])
    expect(lists.bookkeeping).toEqual(['scheduled', 'merchants'])
  })

  it('空种子组（洞察）同样追加：移入即非空，「更多」链接的渲染条件（清单非空）即刻满足', () => {
    expect(moveIntoContainment(lists, 'insights', 'search').insights).toEqual(['search'])
  })

  it('重复移入去重保位：已在清单尾的成员再移入内容不变', () => {
    const once = moveIntoContainment(lists, 'bookkeeping', 'transactions')
    expect(moveIntoContainment(once, 'bookkeeping', 'transactions')).toEqual(once)
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

describe('右键「移入更多」写路径（issue #474：点选即追加本组清单尾、双存储立即持久化、键位随动）', () => {
  const ORDER_KEY = VIEW_STATE_KEYS.sidebarOrder
  const CONTAINMENT_KEY = VIEW_STATE_KEYS.sidebarContainment

  beforeEach(() => {
    localStorage.clear()
    vi.resetModules()
  })

  afterEach(() => {
    localStorage.removeItem(ORDER_KEY)
    localStorage.removeItem(CONTAINMENT_KEY)
    vi.resetModules()
  })

  async function fresh() {
    return await import('@/composables/useViewShortcuts')
  }

  it('applyMoveIntoMore：主项退出组内序 + 追加本组收纳清单尾，双存储立即持久化', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    expect(mod.sidebarGroupOrders.value.bookkeeping).toEqual(['accounts', 'budget'])
    expect(mod.sidebarContainment.value.bookkeeping).toEqual(['scheduled', 'merchants', 'transactions'])
    expect(JSON.parse(localStorage.getItem(ORDER_KEY)!)).toEqual(mod.sidebarGroupOrders.value)
    expect(JSON.parse(localStorage.getItem(CONTAINMENT_KEY)!)).toEqual(mod.sidebarContainment.value)
  })

  it('键位随动重排：本组带内前移（他组键位不受牵连），移入成员退出键位表（不可键盘触发）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'accounts')!.key).toBe('1')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('4')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'transactions')).toBeUndefined()
  })

  it('移入空组（洞察）：清单即刻非空（侧栏「更多」链接渲染条件满足）、组内序同步收缩', async () => {
    const mod = await fresh()
    expect(mod.sidebarContainment.value.insights).toEqual([])
    mod.applyMoveIntoMore('reports')
    expect(mod.sidebarContainment.value.insights).toEqual(['reports'])
    expect(mod.sidebarGroupOrders.value.insights).toEqual(['search'])
  })

  it('固定项与收纳成员不可移入：no-op 不写任何存储', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('dashboard')
    mod.applyMoveIntoMore('ai')
    mod.applyMoveIntoMore('settings')
    mod.applyMoveIntoMore('scheduled' as ViewName)
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
    expect(mod.sidebarGroupOrders.value).toEqual(mod.parseGroupOrders(null))
  })

  it('重启（重导入）后移入保持：主项不复活、清单保持、键位保持重排（持久化往返）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    vi.resetModules()
    const rebooted = await fresh()
    expect(rebooted.sidebarGroupOrders.value.bookkeeping).toEqual(['accounts', 'budget'])
    expect(rebooted.sidebarContainment.value.bookkeeping).toEqual(['scheduled', 'merchants', 'transactions'])
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'accounts')!.key).toBe('1')
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'transactions')).toBeUndefined()
  })

  it('恢复默认排序复位移入：双存储清空、主项回组内、清单回种子（一键回出厂唯一通道）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    mod.resetSidebarOrder()
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
    expect(mod.sidebarGroupOrders.value).toEqual(mod.parseGroupOrders(null))
    expect(mod.sidebarContainment.value.bookkeeping).toEqual(['scheduled', 'merchants'])
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

// ---------------------------------------------------------------------------
// 右键「移回侧栏」（issue #475 / ADR-0063 决策 2/4）：组满置灰判定、移回纯函数、
// 页签右键菜单构建、写路径（清单删除 + 主项落末位、双存储立即持久化、键位随动）。
// 移回的出厂种子以主项身份在册：解析不复活回收纳清单、键位重排、可再排序/再移入。
// ---------------------------------------------------------------------------

describe('isGroupFull（组满置灰判定纯函数，issue #475 / ADR-0063 决策 2：每组主项 ≤3 运行时硬上限）', () => {
  it('上限常量 = 3：三组 × 3 即键位带封闭性的本体', () => {
    expect(GROUP_MAIN_LIMIT).toBe(3)
  })

  it('主项数达 3 即满：记账出厂满员（定时/商户移回置灰的判定面），两员组未满，空组未满', () => {
    expect(isGroupFull(DEFAULT_ORDERS.bookkeeping)).toBe(true)
    expect(isGroupFull(DEFAULT_ORDERS.assets)).toBe(false)
    expect(isGroupFull(DEFAULT_ORDERS.insights)).toBe(false)
    expect(isGroupFull([])).toBe(false)
  })
})

describe('moveBackToSidebar（移回纯函数，issue #475：移回 = 清单删除，主项归属由写路径落位）', () => {
  const lists: SidebarContainmentLists = {
    bookkeeping: ['scheduled', 'merchants'],
    assets: ['policies', 'physicalAssets'],
    insights: [],
  }

  it('从本组清单删除成员：他组不受牵连、输入不改', () => {
    const next = moveBackToSidebar(lists, 'bookkeeping', 'scheduled')
    expect(next.bookkeeping).toEqual(['merchants'])
    expect(next.assets).toEqual(['policies', 'physicalAssets'])
    expect(next.insights).toEqual([])
    expect(lists.bookkeeping).toEqual(['scheduled', 'merchants'])
  })

  it('成员不在清单（含空清单组）：原样返回内容（no-op 语义）', () => {
    expect(moveBackToSidebar(lists, 'insights', 'search')).toEqual(lists)
    expect(moveBackToSidebar(lists, 'bookkeeping', 'transactions')).toEqual(lists)
  })
})

describe('buildTabContextMenuOptions（「更多」页页签右键菜单构建纯函数，issue #475）', () => {
  it('组未满：「移回侧栏」可用（单菜单项）', () => {
    const opts = buildTabContextMenuOptions(['investments', 'items'])
    expect(opts).toHaveLength(1)
    expect(opts[0]!.key).toBe('backToSidebar')
    expect(opts[0]!.disabled).toBe(false)
  })

  it('组满 3 主项：「移回侧栏」置灰且不隐藏菜单项（上限可见、可学习），提示文案挂在标签渲染函数里', () => {
    const opts = buildTabContextMenuOptions(['transactions', 'accounts', 'budget'])
    expect(opts).toHaveLength(1)
    expect(opts[0]!.key).toBe('backToSidebar')
    expect(opts[0]!.disabled).toBe(true)
    expect(typeof opts[0]!.label).toBe('function')
  })
})

describe('parseContainmentLists × 组内序耦合（issue #475：移回的出厂种子不复活回收纳清单）', () => {
  it('清单数组存在且未列某种子、该种子已在本组组内序中：移回态，不按「缺失种子」补尾', () => {
    const rawOrders = {
      bookkeeping: ['accounts', 'budget', 'scheduled'],
      assets: ['investments', 'items'],
      insights: ['reports', 'search'],
    }
    const parsed = parseContainmentLists(
      { bookkeeping: ['merchants'], assets: ['policies', 'physicalAssets'], insights: [] },
      rawOrders,
    )
    expect(parsed.bookkeeping).toEqual(['merchants'])
  })

  it('清单存储缺失（旧版/出厂）：种子照常补尾（#473 迁移语义不变，存量组内序含定时仍判收纳）', () => {
    const rawOrders = { bookkeeping: ['transactions', 'accounts', 'budget', 'scheduled'] }
    expect(parseContainmentLists(null, rawOrders).bookkeeping).toEqual(['scheduled', 'merchants'])
  })

  it('清单数组存在但种子既不在清单也不在组内序（脏数据丢失）：照常补尾回出厂', () => {
    const rawOrders = { bookkeeping: ['transactions', 'accounts', 'budget'] }
    expect(parseContainmentLists({ bookkeeping: ['merchants'] }, rawOrders).bookkeeping).toEqual([
      'merchants',
      'scheduled',
    ])
  })

  it('不带第二参数时行为不变（缺失种子照常补尾，既有调用零影响）', () => {
    expect(parseContainmentLists({ bookkeeping: ['merchants'] }).bookkeeping).toEqual(['merchants', 'scheduled'])
  })
})

describe('parseGroupOrders × 移回种子（issue #475：移回的出厂种子是合法主项）', () => {
  const contained: SidebarContainmentLists = {
    bookkeeping: ['merchants'],
    assets: ['policies', 'physicalAssets'],
    insights: [],
  }

  it('组内序中的本组种子（未在清单）按主项保留原位，不按非法名过滤；缺失主项照常补尾', () => {
    expect(parseGroupOrders({ bookkeeping: ['budget', 'scheduled', 'accounts', 'transactions'] }, contained).bookkeeping)
      .toEqual(['budget', 'scheduled', 'accounts', 'transactions'])
  })

  it('仍在清单中的种子依旧是非法名（清单是成员资格唯一事实源，#474 语义不变）；缺失主项照常补尾', () => {
    expect(parseGroupOrders({ bookkeeping: ['transactions', 'scheduled'] }, {
      ...contained,
      bookkeeping: ['scheduled', 'merchants'],
    }).bookkeeping).toEqual(['transactions', 'accounts', 'budget'])
  })
})

describe('右键「移回侧栏」写路径（issue #475：点选即清单删除、主项落末位、双存储立即持久化、键位随动）', () => {
  const ORDER_KEY = VIEW_STATE_KEYS.sidebarOrder
  const CONTAINMENT_KEY = VIEW_STATE_KEYS.sidebarContainment

  beforeEach(() => {
    localStorage.clear()
    vi.resetModules()
  })

  afterEach(() => {
    localStorage.removeItem(ORDER_KEY)
    localStorage.removeItem(CONTAINMENT_KEY)
    vi.resetModules()
  })

  async function fresh() {
    return await import('@/composables/useViewShortcuts')
  }

  it('组未满时移回：种子退出收纳清单 + 落本组主项末位，双存储立即持久化', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions') // 腾位：记账组主项剩 2（出厂满员须先移出一个主项）
    mod.applyMoveBackToSidebar('scheduled')
    expect(mod.sidebarContainment.value.bookkeeping).toEqual(['merchants', 'transactions'])
    expect(mod.sidebarGroupOrders.value.bookkeeping).toEqual(['accounts', 'budget', 'scheduled'])
    expect(JSON.parse(localStorage.getItem(ORDER_KEY)!)).toEqual(mod.sidebarGroupOrders.value)
    expect(JSON.parse(localStorage.getItem(CONTAINMENT_KEY)!)).toEqual(mod.sidebarContainment.value)
  })

  it('键位随动重排：移回成员落本组带内末位（账户=1、预算=2、定时=3），他组键位不受牵连（投资=4）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    mod.applyMoveBackToSidebar('scheduled')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'accounts')!.key).toBe('1')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'budget')!.key).toBe('2')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'scheduled')!.key).toBe('3')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('4')
  })

  it('组满拒写（运行时硬上限兑底，菜单置灰为第一道防线）：满员组移回 no-op 不写存储', async () => {
    const mod = await fresh()
    mod.applyMoveBackToSidebar('scheduled') // 记账出厂满员
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
    expect(mod.sidebarContainment.value.bookkeeping).toEqual(['scheduled', 'merchants'])
  })

  it('非清单成员（在册主项、固定项）no-op 不写存储', async () => {
    const mod = await fresh()
    mod.applyMoveBackToSidebar('reports')
    mod.applyMoveBackToSidebar('dashboard' as ContainableViewName)
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
  })

  it('移回资产组成员：保单落资产组主项末位（出厂未满员，无需腾位）', async () => {
    const mod = await fresh()
    mod.applyMoveBackToSidebar('policies')
    expect(mod.sidebarContainment.value.assets).toEqual(['physicalAssets'])
    expect(mod.sidebarGroupOrders.value.assets).toEqual(['investments', 'items', 'policies'])
  })

  it('移回组内最后一个收纳成员后清单为空（侧栏「更多」链接渲染条件失效）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('reports')
    mod.applyMoveIntoMore('search')
    mod.applyMoveBackToSidebar('reports')
    mod.applyMoveBackToSidebar('search')
    expect(mod.sidebarContainment.value.insights).toEqual([])
  })

  it('移回的种子可再移入更多、可组内排序微调（主项词表对称，ADR-0063 决策 4 无例外清单）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    mod.applyMoveBackToSidebar('scheduled')
    mod.applySidebarSort('scheduled', 'top')
    expect(mod.sidebarGroupOrders.value.bookkeeping).toEqual(['scheduled', 'accounts', 'budget'])
    mod.applyMoveIntoMore('scheduled')
    expect(mod.sidebarGroupOrders.value.bookkeeping).toEqual(['accounts', 'budget'])
    expect(mod.sidebarContainment.value.bookkeeping).toEqual(['merchants', 'transactions', 'scheduled'])
  })

  it('重启（重导入）后移回保持：种子不复活回收纳清单、主项保持、键位保持（持久化往返）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    mod.applyMoveBackToSidebar('scheduled')
    vi.resetModules()
    const rebooted = await fresh()
    expect(rebooted.sidebarGroupOrders.value.bookkeeping).toEqual(['accounts', 'budget', 'scheduled'])
    expect(rebooted.sidebarContainment.value.bookkeeping).toEqual(['merchants', 'transactions'])
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'scheduled')!.key).toBe('3')
  })

  it('恢复默认排序复位移回：种子回收纳清单、主项回出厂（一键回出厂唯一通道不变）', async () => {
    const mod = await fresh()
    mod.applyMoveIntoMore('transactions')
    mod.applyMoveBackToSidebar('scheduled')
    mod.resetSidebarOrder()
    expect(mod.sidebarGroupOrders.value.bookkeeping).toEqual(['transactions', 'accounts', 'budget'])
    expect(mod.sidebarContainment.value.bookkeeping).toEqual(['scheduled', 'merchants'])
  })

  it('isSidebarMember（在册判定）：默认在册 = 七主项；移回种子后在册、移入后退出在册；固定项恒不在册', async () => {
    const mod = await fresh()
    expect(mod.isSidebarMember('transactions')).toBe(true)
    expect(mod.isSidebarMember('scheduled')).toBe(false)
    mod.applyMoveIntoMore('transactions')
    expect(mod.isSidebarMember('transactions')).toBe(false)
    mod.applyMoveBackToSidebar('scheduled')
    expect(mod.isSidebarMember('scheduled')).toBe(true)
    expect(mod.isSidebarMember('dashboard')).toBe(false)
    expect(mod.isSidebarMember('bogus')).toBe(false)
  })

  it('isViewContained（收纳在册判定）：出厂种子在册；移回后出册（/policies 路由守卫消费面）', async () => {
    const mod = await fresh()
    expect(mod.isViewContained('policies')).toBe(true)
    mod.applyMoveBackToSidebar('policies')
    expect(mod.isViewContained('policies')).toBe(false)
    expect(mod.isViewContained('physicalAssets')).toBe(true)
  })
})
