import { describe, it, expect, vi, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import type { Router } from 'vue-router'
import AppDropdown from '@/components/AppDropdown.vue'
import { createOverlayToken, openOverlayNames, resetOverlays } from '@/composables/overlayRegistry'
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
  isArrangeableView,
  isSidebarSortAction,
  moveArrangeable,
  buildSidebarSortMenuOptions,
} from '@/composables/useViewShortcuts'
import type { DropdownOption } from 'naive-ui'
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

  it('读取为空时回退默认序（未自定义或恢复默认后存储为空）', async () => {
    vi.resetModules()
    const mod = await import('@/composables/useViewShortcuts')
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
    expect(mod.viewShortcuts.value.find((s) => s.name === 'investments')!.key).toBe('5')
  })
})

/** 菜单选项测试取值助手：把 DropdownOption 收窄到本菜单用到的字段 */
function row(o: DropdownOption) {
  return o as { label?: string; key?: string; disabled?: boolean; type?: string }
}

describe('moveArrangeable（可排区移动纯函数，issue #270）', () => {
  const base = [...ARRANGEABLE_VIEWS]

  it('上移一位：与前一项交换，其余相对顺序不变', () => {
    expect(moveArrangeable(base, 'accounts', 'up')).toEqual([
      'accounts', 'transactions', 'budget', 'investments', 'reports', 'scheduled', 'items', 'search',
    ])
  })

  it('下移一位：与后一项交换，其余相对顺序不变', () => {
    expect(moveArrangeable(base, 'accounts', 'down')).toEqual([
      'transactions', 'budget', 'accounts', 'investments', 'reports', 'scheduled', 'items', 'search',
    ])
  })

  it('移到顶部（可排区第 2 位）', () => {
    expect(moveArrangeable(base, 'scheduled', 'top')[0]).toBe('scheduled')
    expect(moveArrangeable(base, 'scheduled', 'top').slice(1)).toEqual([
      'transactions', 'accounts', 'budget', 'investments', 'reports', 'items', 'search',
    ])
  })

  it('移到底部（可排区第 9 位）', () => {
    const result = moveArrangeable(base, 'budget', 'bottom')
    expect(result[result.length - 1]).toBe('budget')
    expect(result.slice(0, -1)).toEqual([
      'transactions', 'accounts', 'investments', 'reports', 'scheduled', 'items', 'search',
    ])
  })

  it('边界 no-op：首位上移/移顶、末位下移/移底内容不变，且返回新数组不改输入', () => {
    expect(moveArrangeable(base, 'transactions', 'up')).toEqual(base)
    expect(moveArrangeable(base, 'transactions', 'top')).toEqual(base)
    expect(moveArrangeable(base, 'search', 'down')).toEqual(base)
    expect(moveArrangeable(base, 'search', 'bottom')).toEqual(base)
    const upResult = moveArrangeable(base, 'transactions', 'up')
    expect(upResult).not.toBe(base)
    expect(base).toEqual([...ARRANGEABLE_VIEWS])
  })

  it('在自定义序上同样正确：已置顶项再上移为 no-op', () => {
    const custom = moveArrangeable(base, 'search', 'top')
    expect(moveArrangeable(custom, 'search', 'up')).toEqual(custom)
  })

  it('目标项不在序中（如固定项名）：原样返回内容', () => {
    expect(moveArrangeable(base, 'ai', 'up')).toEqual(base)
  })
})

describe('buildSidebarSortMenuOptions（排序菜单选项构建纯函数，含边界置灰，issue #270）', () => {
  const base = [...ARRANGEABLE_VIEWS]

  it('菜单形状：四种移动 + 分隔线 + 恢复默认排序，key 固定', () => {
    const opts = buildSidebarSortMenuOptions('accounts', base)
    expect(opts.map(row).map((o) => o.label ?? o.type)).toEqual([
      '上移一位', '下移一位', '移到顶部', '移到底部', 'divider', '恢复默认排序',
    ])
    expect(opts.map(row).map((o) => o.key)).toEqual([
      'up', 'down', 'top', 'bottom', 'sort-divider', 'reset',
    ])
  })

  it('菜单 key 与移动动作同一词表（防两套词表错位回归：菜单里的移动 key 必须全是合法 SidebarSortAction）', () => {
    const moveKeys = buildSidebarSortMenuOptions('budget', base)
      .map(row)
      .map((o) => o.key!)
      .filter((k) => k !== 'sort-divider' && k !== 'reset')
    expect(moveKeys).toEqual(['up', 'down', 'top', 'bottom'])
    for (const k of moveKeys) expect(isSidebarSortAction(k)).toBe(true)
    // 每个菜单 key 直接可作为动作施加，效果与菜单语义一致（key 即 action）
    expect(moveArrangeable(base, 'accounts', 'up')[0]).toBe('accounts')
    expect(moveArrangeable(base, 'budget', 'bottom').at(-1)).toBe('budget')
  })

  it('可排区首位项：上移/移顶置灰，下移/移底可用', () => {
    const [up, down, top, bottom] = buildSidebarSortMenuOptions('transactions', base).map(row)
    expect(up!.disabled).toBe(true)
    expect(top!.disabled).toBe(true)
    expect(down!.disabled).toBe(false)
    expect(bottom!.disabled).toBe(false)
  })

  it('可排区末位项：下移/移底置灰，上移/移顶可用', () => {
    const [up, down, top, bottom] = buildSidebarSortMenuOptions('search', base).map(row)
    expect(down!.disabled).toBe(true)
    expect(bottom!.disabled).toBe(true)
    expect(up!.disabled).toBe(false)
    expect(top!.disabled).toBe(false)
  })

  it('中间项：四种移动全部可用；恢复默认排序恒可用', () => {
    const opts = buildSidebarSortMenuOptions('budget', base).map(row)
    for (const o of opts) {
      if (o.type === 'divider') continue
      expect(o.disabled).toBe(false)
    }
    expect(opts[5]!.key).toBe('reset')
    expect(opts[5]!.disabled).toBe(false)
  })

  it('自定义序下的边界按当前序判定', () => {
    const custom = moveArrangeable(base, 'search', 'top')
    const [up, , top] = buildSidebarSortMenuOptions('search', custom).map(row)
    expect(up!.disabled).toBe(true)
    expect(top!.disabled).toBe(true)
  })
})

describe('isArrangeableView（固定项例外判定，issue #270）', () => {
  it('可排区八项为真；概览/AI/设置三固定项为假', () => {
    for (const name of ARRANGEABLE_VIEWS) expect(isArrangeableView(name)).toBe(true)
    expect(isArrangeableView('dashboard')).toBe(false)
    expect(isArrangeableView('ai')).toBe(false)
    expect(isArrangeableView('settings')).toBe(false)
    expect(isArrangeableView('bogus')).toBe(false)
  })
})

describe('写路径与持久化（issue #270：点选即重排、立即持久化、重启保持、恢复默认）', () => {
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

  it('applySidebarSort：响应式重排 + 立即持久化', async () => {
    const mod = await fresh()
    mod.applySidebarSort('search', 'top')
    expect(mod.sidebarOrder.value[0]).toBe('search')
    expect(mod.viewShortcuts.value.map((s) => s.name)[1]).toBe('search')
    expect(localStorage.getItem(ORDER_KEY)).toBe(JSON.stringify([...mod.sidebarOrder.value]))
  })

  it('重启（重导入）后自定义序保持，键位与侧栏序随新位置一致', async () => {
    const mod = await fresh()
    mod.applySidebarSort('search', 'top')
    vi.resetModules()
    const rebooted = await fresh()
    expect(rebooted.viewShortcuts.value.map((s) => s.name)).toEqual([
      'dashboard', 'search', 'transactions', 'accounts', 'budget', 'investments', 'reports', 'scheduled', 'items', 'ai', 'settings',
    ])
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'search')!.key).toBe('2')
    expect(rebooted.viewShortcuts.value.find((s) => s.name === 'items')!.key).toBe('9')
  })

  it('save→load→解析往返：持久化值经 parseArrangeableOrder 防御后还原', async () => {
    const mod = await fresh()
    mod.applySidebarSort('items', 'top')
    mod.applySidebarSort('search', 'up')
    // 直接读存储做解析往返（等价启动读路径）
    const raw = JSON.parse(localStorage.getItem(ORDER_KEY)!)
    expect(mod.parseArrangeableOrder(raw)).toEqual([...mod.sidebarOrder.value])
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
    expect(mod.sidebarOrder.value[mod.sidebarOrder.value.length - 1]).toBe('accounts')
    // 再恢复，仍回默认
    mod.resetSidebarOrder()
    expect(mod.viewShortcuts.value.map((s) => s.name)).toEqual([...mod.DEFAULT_VIEW_ORDER])
  })

  it('边界移动（首位上移）不改顺序且不写存储：保住「恢复默认 = 删 key」语义，出厂序调整时自动跟随', async () => {
    const mod = await fresh()
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    mod.applySidebarSort('transactions', 'up')
    expect(mod.sidebarOrder.value).toEqual([...mod.ARRANGEABLE_VIEWS])
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
