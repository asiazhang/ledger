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
  sidebarGroupOrders,
} from '@/composables/useViewShortcuts'
import type { ViewName, SidebarGroupOrders } from '@/composables/useViewShortcuts'
import type { DropdownOption } from 'naive-ui'
import { VIEW_STATE_KEYS } from '@/utils/view-state'

function setPlatform(platform: string) {
  Object.defineProperty(navigator, 'platform', { value: platform, configurable: true })
}

function press(key: string, mods: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean } = {}): KeyboardEvent {
  return new KeyboardEvent('keydown', { key, bubbles: true, ...mods })
}

afterEach(() => setPlatform(''))

describe('侧栏分组常量（issue #359 / ADR-0051）', () => {
  it('三组锁定：记账 = 交易、账户、预算、定时；资产 = 投资、物品、保单；洞察 = 报表、搜索', () => {
    expect(SIDEBAR_GROUPS.map((g) => g.id)).toEqual(['bookkeeping', 'assets', 'insights'])
    expect(SIDEBAR_GROUPS.map((g) => [...g.views])).toEqual([
      ['transactions', 'accounts', 'budget', 'scheduled'],
      ['investments', 'items', 'policies'],
      ['reports', 'search'],
    ])
  })

  it('线性默认序：概览 + 各组按组序展开 + AI + 设置（分组标题不占位、不计数）', () => {
    expect([...DEFAULT_VIEW_ORDER]).toEqual([
      'dashboard',
      'transactions',
      'accounts',
      'budget',
      'scheduled',
      'investments',
      'items',
      'policies',
      'reports',
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

  it('可排区 = 各组成员按组序展开共 8 项', () => {
    expect(ARRANGEABLE_VIEWS).toEqual([
      'transactions',
      'accounts',
      'budget',
      'scheduled',
      'investments',
      'items',
      'policies',
      'reports',
      'search',
    ])
  })

  it('groupOfView：可排区九项各有其组；概览/AI/设置与未知名不在任何组', () => {
    expect(groupOfView('transactions')).toBe('bookkeeping')
    expect(groupOfView('scheduled')).toBe('bookkeeping')
    expect(groupOfView('investments')).toBe('assets')
    expect(groupOfView('items')).toBe('assets')
    expect(groupOfView('policies')).toBe('assets')
    expect(groupOfView('reports')).toBe('insights')
    expect(groupOfView('search')).toBe('insights')
    expect(groupOfView('dashboard')).toBeNull()
    expect(groupOfView('ai')).toBeNull()
    expect(groupOfView('settings')).toBeNull()
    expect(groupOfView('bogus' as ViewName)).toBeNull()
  })
})

/** 默认组内序（= SIDEBAR_GROUPS 成员展开，保单为资产组末位）：解析/持久化断言复用 */
const DEFAULT_ORDERS: SidebarGroupOrders = {
  bookkeeping: ['transactions', 'accounts', 'budget', 'scheduled'],
  assets: ['investments', 'items', 'policies'],
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
    const legacy = ['transactions', 'accounts', 'budget', 'investments', 'reports', 'scheduled', 'items', 'search']
    expect(parseGroupOrders(legacy)).toEqual(defaults)
  })

  it('组内已存顺序保留；非法名（固定项名、他组成员、未知名、非字符串）被过滤，缺失项按默认序补入组内末尾', () => {
    expect(parseGroupOrders({
      bookkeeping: ['budget', 'transactions', 'dashboard', 'investments', 'bogus', 42, 'accounts', 'ai'],
      assets: ['items'],
      insights: 'junk',
    })).toEqual({
      bookkeeping: ['budget', 'transactions', 'accounts', 'scheduled'],
      assets: ['items', 'investments', 'policies'],
      insights: ['reports', 'search'],
    })
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
      bookkeeping: ['scheduled', 'budget', 'accounts', 'transactions'],
      assets: defaults.assets,
      insights: ['search', 'reports'],
    })
  })
})

describe('viewShortcuts 派生（键位按线性位置推导，issue #359）', () => {
  it('数字键 1..9 连续无空洞、第 9 个可排项无键位、AI=0、设置为逗号键', () => {
    expect(viewShortcuts.value.map((s) => s.key)).toEqual([
      '1', '2', '3', '4', '5', '6', '7', '8', '9', null, '0', ',',
    ])
    expect(viewShortcuts.value.map((s) => s.name)).toEqual([...DEFAULT_VIEW_ORDER])
  })

  it('每个侧栏视图恰一条记录；分组形态下键位随组序：定时=5、投资=6、物品=7、保单=8、报表=9、搜索无键位、AI=0', () => {
    const names = viewShortcuts.value.map((s) => s.name)
    expect(new Set(names).size).toBe(names.length)
    expect(new Set(names)).toEqual(new Set(DEFAULT_VIEW_ORDER))
    expect(viewShortcuts.value.find((v) => v.name === 'settings')!.key).toBe(',')
    expect(viewShortcuts.value.find((v) => v.name === 'scheduled')!.key).toBe('5')
    expect(viewShortcuts.value.find((v) => v.name === 'investments')!.key).toBe('6')
    expect(viewShortcuts.value.find((v) => v.name === 'items')!.key).toBe('7')
    expect(viewShortcuts.value.find((v) => v.name === 'policies')!.key).toBe('8')
    expect(viewShortcuts.value.find((v) => v.name === 'reports')!.key).toBe('9')
    expect(viewShortcuts.value.find((v) => v.name === 'search')!.key).toBeNull()
    expect(viewShortcuts.value.find((v) => v.name === 'ai')!.key).toBe('0')
  })

  it('deriveViewShortcuts：可排区第 9 项落在键位带之外——末位无键位（key: null，默认序即搜索）', () => {
    // 保单已接入（issue #360）：默认序第 7 位（保单）得 ⌘8、第 8 位（报表）得 ⌘9，
    // 第 9 位（洞察组末位 = 搜索）无键位
    const shortcuts = deriveViewShortcuts(DEFAULT_ORDERS)
    expect(shortcuts.find((s) => s.name === 'policies')!.key).toBe('8')
    expect(shortcuts.find((s) => s.name === 'reports')!.key).toBe('9')
    expect(shortcuts.find((s) => s.name === 'search')!.key).toBeNull()
    expect(shortcuts.find((s) => s.name === 'ai')!.key).toBe('0')
  })

  it('无键位视图不可经键盘触发：数字键位恰 10 条非空记录；重排即换谁无键位', () => {
    const shortcuts = deriveViewShortcuts(DEFAULT_ORDERS)
    const numberKeyed = shortcuts.filter((s) => s.key !== null && s.key !== ',')
    expect(numberKeyed).toHaveLength(10) // 概览 + 可排区前八项 + AI：数字键物理上限
    expect(numberKeyed.map((s) => s.name)).not.toContain('search')
    // 组内重排把搜索上移一位后，报表落到末位、变成无键位的那个
    const reordered = deriveViewShortcuts({ ...DEFAULT_ORDERS, insights: ['search', 'reports'] })
    expect(reordered.find((s) => s.name === 'search')!.key).toBe('9')
    expect(reordered.find((s) => s.name === 'reports')!.key).toBeNull()
  })

  it('组内重排即重排键位（键随线性位置）', () => {
    const reordered: SidebarGroupOrders = { ...DEFAULT_ORDERS, bookkeeping: ['budget', 'transactions', 'accounts', 'scheduled'] }
    const shortcuts = deriveViewShortcuts(reordered)
    expect(shortcuts.find((s) => s.name === 'budget')!.key).toBe('2')
    expect(shortcuts.find((s) => s.name === 'scheduled')!.key).toBe('5')
  })
})

describe('matchViewShortcut', () => {
  it('macOS 上 Cmd+数字命中对应视图（键位严格随分组形态的线性位置）', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1', { metaKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('2', { metaKey: true }))).toBe('transactions')
    expect(matchViewShortcut(press('3', { metaKey: true }))).toBe('accounts')
    expect(matchViewShortcut(press('4', { metaKey: true }))).toBe('budget')
    expect(matchViewShortcut(press('5', { metaKey: true }))).toBe('scheduled')
    expect(matchViewShortcut(press('6', { metaKey: true }))).toBe('investments')
    expect(matchViewShortcut(press('7', { metaKey: true }))).toBe('items')
    expect(matchViewShortcut(press('8', { metaKey: true }))).toBe('policies')
    expect(matchViewShortcut(press('9', { metaKey: true }))).toBe('reports')
    expect(matchViewShortcut(press('0', { metaKey: true }))).toBe('ai')
    expect(matchViewShortcut(press(',', { metaKey: true }))).toBe('settings')
  })

  it('无键位视图（可排区末位）不可经键盘触发', () => {
    setPlatform('MacIntel')
    // 默认序末位 = 搜索：不出提示、按键不命中（右键重排可换谁无键位）
    expect(matchViewShortcut(press('9', { metaKey: true }))).not.toBe('search')
  })

  it('macOS 上 Ctrl+数字不命中（需要 Cmd）', () => {
    setPlatform('MacIntel')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBeNull()
  })

  it('非 macOS 上 Ctrl+数字命中对应视图', () => {
    setPlatform('Win32')
    expect(matchViewShortcut(press('1', { ctrlKey: true }))).toBe('dashboard')
    expect(matchViewShortcut(press('3', { ctrlKey: true }))).toBe('accounts')
    expect(matchViewShortcut(press('6', { ctrlKey: true }))).toBe('investments')
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

  it('弹层上报打开时抑制跳转（注册表信号，ADR-0035）', () => {
    setPlatform('MacIntel')
    const { router, push } = makeRouter('dashboard')
    mountHost(router)
    const token = createOverlayToken('modal')
    token.set(true)
    window.dispatchEvent(press('4', { metaKey: true }))
    expect(push).not.toHaveBeenCalled()
    token.set(false)
    window.dispatchEvent(press('4', { metaKey: true }))
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

  it('已存组内序对象：各组独立解析生效，键位随组内线性位置', async () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify({ bookkeeping: ['scheduled', 'transactions'], insights: ['search', 'reports'] }),
    )
    vi.resetModules()
    const mod = await import('@/composables/useViewShortcuts')
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([
      'dashboard',
      'scheduled',
      'transactions',
      'accounts',
      'budget',
      'investments',
      'items',
      'policies',
      'search',
      'reports',
      'ai',
      'settings',
    ])
    expect(mod.viewShortcuts.value.find((s) => s.name === 'scheduled')!.key).toBe('2')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'transactions')!.key).toBe('3')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'policies')!.key).toBe('8')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('9')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'reports')!.key).toBeNull()
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
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('6')
  })
})

/** 菜单选项测试取值助手：把 DropdownOption 收窄到本菜单用到的字段 */
function row(o: DropdownOption) {
  return o as { label?: string; key?: string; disabled?: boolean; type?: string }
}

describe('moveArrangeable（组内移动纯函数，issue #270/#359）', () => {
  const bookkeeping: ViewName[] = ['transactions', 'accounts', 'budget', 'scheduled']

  it('组内上移一位：与前一项交换，其余相对顺序不变', () => {
    expect(moveArrangeable(bookkeeping, 'accounts', 'up')).toEqual([
      'accounts', 'transactions', 'budget', 'scheduled',
    ])
  })

  it('组内下移一位：与后一项交换，其余相对顺序不变', () => {
    expect(moveArrangeable(bookkeeping, 'accounts', 'down')).toEqual([
      'transactions', 'budget', 'accounts', 'scheduled',
    ])
  })

  it('移到组内顶部', () => {
    expect(moveArrangeable(bookkeeping, 'budget', 'top')).toEqual([
      'budget', 'transactions', 'accounts', 'scheduled',
    ])
  })

  it('移到组内底部', () => {
    expect(moveArrangeable(bookkeeping, 'accounts', 'bottom')).toEqual([
      'transactions', 'budget', 'scheduled', 'accounts',
    ])
  })

  it('组内边界 no-op：首位上移/移顶、末位下移/移底内容不变，且返回新数组不改输入', () => {
    expect(moveArrangeable(bookkeeping, 'transactions', 'up')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'transactions', 'top')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'scheduled', 'down')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'scheduled', 'bottom')).toEqual(bookkeeping)
    const upResult = moveArrangeable(bookkeeping, 'transactions', 'up')
    expect(upResult).not.toBe(bookkeeping)
    expect(bookkeeping).toEqual(['transactions', 'accounts', 'budget', 'scheduled'])
  })

  it('在自定义序上同样正确：已置顶项再上移为 no-op', () => {
    const custom = moveArrangeable(bookkeeping, 'scheduled', 'top')
    expect(moveArrangeable(custom, 'scheduled', 'up')).toEqual(custom)
  })

  it('目标项不在序中（如固定项名、他组成员）：原样返回内容——移动只见一张组内数组，跨组移动结构上不可达', () => {
    expect(moveArrangeable(bookkeeping, 'ai', 'up')).toEqual(bookkeeping)
    expect(moveArrangeable(bookkeeping, 'search', 'bottom')).toEqual(bookkeeping)
  })
})

describe('buildSidebarSortMenuOptions（排序菜单选项构建纯函数，含组内边界置灰，issue #270/#359）', () => {
  const bookkeeping: ViewName[] = ['transactions', 'accounts', 'budget', 'scheduled']

  it('菜单形状：四种移动 + 分隔线 + 恢复默认排序，key 固定', () => {
    const opts = buildSidebarSortMenuOptions('accounts', bookkeeping)
    expect(opts.map(row).map((o) => o.label ?? o.type)).toEqual([
      '上移一位', '下移一位', '移到顶部', '移到底部', 'divider', '恢复默认排序',
    ])
    expect(opts.map(row).map((o) => o.key)).toEqual([
      'up', 'down', 'top', 'bottom', 'sort-divider', 'reset',
    ])
  })

  it('菜单 key 与移动动作同一词表（防两套词表错位回归：菜单里的移动 key 必须全是合法 SidebarSortAction）', () => {
    const moveKeys = buildSidebarSortMenuOptions('budget', bookkeeping)
      .map(row)
      .map((o) => o.key!)
      .filter((k) => k !== 'sort-divider' && k !== 'reset')
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
    const [up, down, top, bottom] = buildSidebarSortMenuOptions('scheduled', bookkeeping).map(row)
    expect(down!.disabled).toBe(true)
    expect(bottom!.disabled).toBe(true)
    expect(up!.disabled).toBe(false)
    expect(top!.disabled).toBe(false)
  })

  it('组内中间项：四种移动全部可用；恢复默认排序恒可用', () => {
    const opts = buildSidebarSortMenuOptions('budget', bookkeeping).map(row)
    for (const o of opts) {
      if (o.type === 'divider') continue
      expect(o.disabled).toBe(false)
    }
    expect(opts[5]!.key).toBe('reset')
    expect(opts[5]!.disabled).toBe(false)
  })

  it('自定义序下的边界按当前组内序判定', () => {
    const custom = moveArrangeable(bookkeeping, 'scheduled', 'top')
    const [up, , top] = buildSidebarSortMenuOptions('scheduled', custom).map(row)
    expect(up!.disabled).toBe(true)
    expect(top!.disabled).toBe(true)
  })
})

describe('isArrangeableView（固定项例外判定，issue #270/#359）', () => {
  it('可排区九项为真；概览/AI/设置三固定项为假', () => {
    for (const name of ARRANGEABLE_VIEWS) expect(isArrangeableView(name)).toBe(true)
    expect(isArrangeableView('dashboard')).toBe(false)
    expect(isArrangeableView('ai')).toBe(false)
    expect(isArrangeableView('settings')).toBe(false)
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
    expect(mod.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('9')
    const stored = JSON.parse(localStorage.getItem(ORDER_KEY)!)
    expect(stored).toEqual({
      bookkeeping: ['transactions', 'accounts', 'budget', 'scheduled'],
      assets: ['investments', 'items', 'policies'],
      insights: ['search', 'reports'],
    })
  })

  it('组内排序不越组：他组数组与他组键位不受牵连', async () => {
    const mod = await fresh()
    mod.applySidebarSort('search', 'top')
    // 他组顺序原样
    expect(mod.sidebarGroupOrders.value.bookkeeping).toEqual(['transactions', 'accounts', 'budget', 'scheduled'])
    expect(mod.sidebarGroupOrders.value.assets).toEqual(['investments', 'items', 'policies'])
    // 他组键位原样（键位带按组连续：记账 2-5、资产 6-8、洞察 9 + 末位无键位）
    expect(mod.viewShortcuts.value.find((s) => s.name === 'transactions')!.key).toBe('2')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'scheduled')!.key).toBe('5')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('6')
    expect(mod.viewShortcuts.value.find((s) => s.name === 'policies')!.key).toBe('8')
    // 本组内键位随动：搜索上移得 9，报表落到可排区末位无键位
    expect(mod.viewShortcuts.value.find((s) => s.name === 'reports')!.key).toBeNull()
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
      'dashboard', 'transactions', 'accounts', 'budget', 'scheduled', 'investments', 'items', 'policies', 'search', 'reports', 'ai', 'settings',
    ])
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('9')
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'reports')!.key).toBeNull()
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
