import { describe, it, expect, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import {
  SIDEBAR_GROUPS,
  DEFAULT_VIEW_ORDER,
  ARRANGEABLE_VIEWS,
  FIRST_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
  GROUP_MAIN_LIMIT,
  GROUP_CONTAINMENT_SEEDS,
  useSidebarOrderStore,
  parseGroupOrders,
  parseContainmentLists,
  moveArrangeable,
  moveIntoContainment,
  moveBackToSidebar,
  isGroupFull,
  isArrangeableView,
  isSidebarSortAction,
  groupOfView,
  buildSidebarSortMenuOptions,
  buildTabContextMenuOptions,
} from '@/stores/sidebar-order'
import type { ViewName, ContainableViewName, SidebarGroupOrders, SidebarContainmentLists } from '@/stores/sidebar-order'
import type { DropdownOption } from 'naive-ui'
import { VIEW_STATE_KEYS, saveContainmentLists } from '@/utils/view-state'

// 侧栏排序 store 接口测试（issue #524/#549：排序状态机迁入 sidebar-order store）。
// 「重启」惯用法 = setActivePinia(createPinia())（sidebar-more-link.test.ts 先例）：
// store 首次实例化即启动读路径（读 view_state:* 两键经解析防御），新 pinia = 新一次启动。
// 键位带推导纯逻辑测试留守 useViewShortcuts.test.ts（经 store 装配），此处不重复。

beforeEach(() => {
  localStorage.clear()
  setActivePinia(createPinia())
})

/** 重启：换新 pinia，下一次 useSidebarOrderStore() 即新一次启动读路径 */
function reboot() {
  setActivePinia(createPinia())
}

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
    expect(groupOfView('insurers')).toBe('assets')
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
    assets: ['policies', 'physicalAssets', 'insurers'],
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

describe('组内收纳清单：出厂种子与解析防御（issue #472/#473 / ADR-0063 决策 3/5）', () => {
  it('出厂种子锁定：记账 = [定时, 商户]（#473）；资产 = [保单, 实物资产, 保司]（#714）；洞察 = 空（出厂不渲染链接）', () => {
    expect(GROUP_CONTAINMENT_SEEDS.bookkeeping).toEqual(['scheduled', 'merchants'])
    expect(GROUP_CONTAINMENT_SEEDS.assets).toEqual(['policies', 'physicalAssets', 'insurers'])
    expect(GROUP_CONTAINMENT_SEEDS.insights).toEqual([])
  })

  it('非对象整体回出厂种子（null/undefined/数组/标量）', () => {
    const seeds = { bookkeeping: ['scheduled', 'merchants'], assets: ['policies', 'physicalAssets', 'insurers'], insights: [] }
    expect(parseContainmentLists(null)).toEqual(seeds)
    expect(parseContainmentLists(undefined)).toEqual(seeds)
    expect(parseContainmentLists(['policies'])).toEqual(seeds)
    expect(parseContainmentLists('x')).toEqual(seeds)
    expect(parseContainmentLists(42)).toEqual(seeds)
  })

  it('各组独立解析：组值非数组该组回种子，他组不受牵连', () => {
    expect(parseContainmentLists({ bookkeeping: [], assets: 'x', insights: [] })).toEqual({
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets', 'insurers'],
      insights: [],
    })
  })

  it('非法名过滤：他组主项/他组种子/固定项名/未知名/非字符串一律不入清单（不跨组）', () => {
    const raw = {
      assets: ['transactions', 'scheduled', 'more', 'dashboard', 'settings', 42, 'bogus', 'policies'],
      bookkeeping: ['policies'],
    }
    const parsed = parseContainmentLists(raw)
    expect(parsed.assets).toEqual(['policies', 'physicalAssets', 'insurers']) // 合法名只剩保单，实物资产与保司作缺失出厂成员补尾
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
    expect(parseContainmentLists({ assets: ['policies', 'policies'] }).assets).toEqual(['policies', 'physicalAssets', 'insurers'])
    expect(parseContainmentLists({ bookkeeping: [] }).bookkeeping).toEqual(['scheduled', 'merchants'])
    expect(parseContainmentLists({ insights: [] }).insights).toEqual([])
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

describe('store 启动读路径（issue #269/#359/#472：读 view_state:* 两键，经解析防御后回默认）', () => {
  it('读取为空时组内序回默认序、收纳清单回出厂种子（未自定义或恢复默认后存储为空）', () => {
    const store = useSidebarOrderStore()
    expect(store.sidebarGroupOrders).toEqual(DEFAULT_ORDERS)
    expect(store.sidebarContainment).toEqual({
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets', 'insurers'],
      insights: [],
    })
  })

  it('已存组内序对象：各组独立解析生效（缺失项补尾、非法名过滤），脏清单回种子', () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify({ bookkeeping: ['budget', 'transactions'], insights: ['search', 'reports'] }),
    )
    localStorage.setItem(VIEW_STATE_KEYS.sidebarContainment, JSON.stringify({ assets: ['merchants'], insights: ['x'] }))
    reboot()
    const store = useSidebarOrderStore()
    expect(store.sidebarGroupOrders).toEqual({
      bookkeeping: ['budget', 'transactions', 'accounts'],
      assets: ['investments', 'items'],
      insights: ['search', 'reports'],
    })
    expect(store.sidebarContainment).toEqual({
      bookkeeping: ['scheduled', 'merchants'],
      assets: ['policies', 'physicalAssets', 'insurers'],
      insights: [],
    })
  })

  it('已存旧平铺排序数据（issue #270 形态）启动不异常、整体回退默认序；标量与损坏 JSON 同样回退', () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify(['search', 'items', 'scheduled', 'reports', 'investments', 'budget', 'accounts', 'transactions']),
    )
    localStorage.setItem(VIEW_STATE_KEYS.sidebarContainment, '"x"')
    reboot()
    const store = useSidebarOrderStore()
    expect(store.sidebarGroupOrders).toEqual(DEFAULT_ORDERS)
    expect(store.sidebarContainment).toEqual(parseContainmentLists(null))
    // 标量 / 损坏 JSON（loadLocal 回 null）同路径
    localStorage.setItem(VIEW_STATE_KEYS.sidebarOrder, '123')
    localStorage.setItem(VIEW_STATE_KEYS.sidebarContainment, 'not-json{')
    reboot()
    expect(useSidebarOrderStore().sidebarGroupOrders).toEqual(DEFAULT_ORDERS)
    expect(useSidebarOrderStore().sidebarContainment).toEqual(parseContainmentLists(null))
  })

  it('存储往返：saveContainmentLists 写入的清单经 parseContainmentLists 防御后还原', () => {
    const lists = { bookkeeping: ['scheduled', 'merchants'], assets: ['policies', 'physicalAssets', 'insurers'], insights: [] }
    saveContainmentLists(lists)
    expect(JSON.parse(localStorage.getItem(VIEW_STATE_KEYS.sidebarContainment)!)).toEqual(lists)
    expect(parseContainmentLists(JSON.parse(localStorage.getItem(VIEW_STATE_KEYS.sidebarContainment)!))).toEqual(lists)
  })

  it('移入/移回状态重启保持（持久化往返）：主项不复活、种子不复活回收纳清单', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    store.applyMoveBackToSidebar('scheduled')
    reboot()
    const rebooted = useSidebarOrderStore()
    expect(rebooted.sidebarGroupOrders.bookkeeping).toEqual(['accounts', 'budget', 'scheduled'])
    expect(rebooted.sidebarContainment.bookkeeping).toEqual(['merchants', 'transactions'])
  })
})

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

/** 菜单选项测试取值助手：把 DropdownOption 收窄到本菜单用到的字段 */
function row(o: DropdownOption) {
  return o as { label?: string; key?: string; disabled?: boolean; type?: string }
}

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

// ---------------------------------------------------------------------------
// 四条写路径：点选即写、双存储同步、边界 no-op 不写（行为等价迁移，纪律不变）。
// ---------------------------------------------------------------------------

describe('applySidebarSort 写路径（issue #270/#359：组内点选即重排、立即持久化、恢复默认）', () => {
  const ORDER_KEY = VIEW_STATE_KEYS.sidebarOrder

  it('组内响应式重排 + 立即持久化（对象形状「组 id → 视图名数组」）', () => {
    const store = useSidebarOrderStore()
    store.applySidebarSort('search', 'top')
    expect(store.sidebarGroupOrders.insights[0]).toBe('search')
    const stored = JSON.parse(localStorage.getItem(ORDER_KEY)!)
    expect(stored).toEqual({
      bookkeeping: ['transactions', 'accounts', 'budget'],
      assets: ['investments', 'items'],
      insights: ['search', 'reports'],
    })
  })

  it('组内排序不越组：他组数组不受牵连', () => {
    const store = useSidebarOrderStore()
    store.applySidebarSort('search', 'top')
    expect(store.sidebarGroupOrders.bookkeeping).toEqual(['transactions', 'accounts', 'budget'])
    expect(store.sidebarGroupOrders.assets).toEqual(['investments', 'items'])
  })

  it('固定项不参与排序（applySidebarSort 对固定项为 no-op，不写存储）', () => {
    const store = useSidebarOrderStore()
    store.applySidebarSort('dashboard', 'bottom')
    store.applySidebarSort('ai', 'top')
    store.applySidebarSort('settings', 'up')
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(store.sidebarGroupOrders).toEqual(parseGroupOrders(null))
  })

  it('重启（新 pinia）后组内自定义序保持', () => {
    const store = useSidebarOrderStore()
    store.applySidebarSort('search', 'top')
    reboot()
    expect(useSidebarOrderStore().sidebarGroupOrders.insights[0]).toBe('search')
  })

  it('save→load→解析往返：持久化值经 parseGroupOrders 防御后还原', () => {
    const store = useSidebarOrderStore()
    store.applySidebarSort('items', 'top')
    store.applySidebarSort('search', 'up')
    // 直接读存储做解析往返（等价启动读路径）
    const raw = JSON.parse(localStorage.getItem(ORDER_KEY)!)
    expect(parseGroupOrders(raw)).toEqual({ ...store.sidebarGroupOrders })
  })

  it('resetSidebarOrder：清除存储回默认序，可反复「自定义 → 恢复 → 再自定义」交替', () => {
    const store = useSidebarOrderStore()
    store.applySidebarSort('items', 'top')
    expect(localStorage.getItem(ORDER_KEY)).not.toBeNull()
    store.resetSidebarOrder()
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(store.sidebarGroupOrders).toEqual(parseGroupOrders(null))
    // 恢复后可再次自定义
    store.applySidebarSort('accounts', 'bottom')
    expect(localStorage.getItem(ORDER_KEY)).not.toBeNull()
    expect(store.sidebarGroupOrders.bookkeeping.at(-1)).toBe('accounts')
    // 再恢复，仍回默认
    store.resetSidebarOrder()
    expect(store.sidebarGroupOrders).toEqual(parseGroupOrders(null))
  })

  it('边界移动（组内首位上移）不改顺序且不写存储：保住「恢复默认 = 删 key」语义，出厂序调整时自动跟随', () => {
    const store = useSidebarOrderStore()
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    store.applySidebarSort('transactions', 'up')
    expect(store.sidebarGroupOrders).toEqual(parseGroupOrders(null))
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
  })
})

describe('右键「移入更多」写路径（issue #474：点选即追加本组清单尾、双存储立即持久化）', () => {
  const ORDER_KEY = VIEW_STATE_KEYS.sidebarOrder
  const CONTAINMENT_KEY = VIEW_STATE_KEYS.sidebarContainment

  it('applyMoveIntoMore：主项退出组内序 + 追加本组收纳清单尾，双存储立即持久化', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    expect(store.sidebarGroupOrders.bookkeeping).toEqual(['accounts', 'budget'])
    expect(store.sidebarContainment.bookkeeping).toEqual(['scheduled', 'merchants', 'transactions'])
    expect(JSON.parse(localStorage.getItem(ORDER_KEY)!)).toEqual({ ...store.sidebarGroupOrders })
    expect(JSON.parse(localStorage.getItem(CONTAINMENT_KEY)!)).toEqual({ ...store.sidebarContainment })
  })

  it('移入空组（洞察）：清单即刻非空（侧栏「更多」链接渲染条件满足）、组内序同步收缩', () => {
    const store = useSidebarOrderStore()
    expect(store.sidebarContainment.insights).toEqual([])
    store.applyMoveIntoMore('reports')
    expect(store.sidebarContainment.insights).toEqual(['reports'])
    expect(store.sidebarGroupOrders.insights).toEqual(['search'])
  })

  it('固定项与收纳成员不可移入：no-op 不写任何存储', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('dashboard')
    store.applyMoveIntoMore('ai')
    store.applyMoveIntoMore('settings')
    store.applyMoveIntoMore('scheduled' as ViewName)
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
    expect(store.sidebarGroupOrders).toEqual(parseGroupOrders(null))
  })

  it('重启（新 pinia）后移入保持：主项不复活、清单保持（持久化往返）', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    reboot()
    const rebooted = useSidebarOrderStore()
    expect(rebooted.sidebarGroupOrders.bookkeeping).toEqual(['accounts', 'budget'])
    expect(rebooted.sidebarContainment.bookkeeping).toEqual(['scheduled', 'merchants', 'transactions'])
  })

  it('恢复默认排序复位移入：双存储清空、主项回组内、清单回种子（一键回出厂唯一通道）', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    store.resetSidebarOrder()
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
    expect(store.sidebarGroupOrders).toEqual(parseGroupOrders(null))
    expect(store.sidebarContainment.bookkeeping).toEqual(['scheduled', 'merchants'])
  })
})

describe('右键「移回侧栏」写路径（issue #475：点选即清单删除、主项落末位、双存储立即持久化）', () => {
  const ORDER_KEY = VIEW_STATE_KEYS.sidebarOrder
  const CONTAINMENT_KEY = VIEW_STATE_KEYS.sidebarContainment

  it('组未满时移回：种子退出收纳清单 + 落本组主项末位，双存储立即持久化', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions') // 腾位：记账组主项剩 2（出厂满员须先移出一个主项）
    store.applyMoveBackToSidebar('scheduled')
    expect(store.sidebarContainment.bookkeeping).toEqual(['merchants', 'transactions'])
    expect(store.sidebarGroupOrders.bookkeeping).toEqual(['accounts', 'budget', 'scheduled'])
    expect(JSON.parse(localStorage.getItem(ORDER_KEY)!)).toEqual({ ...store.sidebarGroupOrders })
    expect(JSON.parse(localStorage.getItem(CONTAINMENT_KEY)!)).toEqual({ ...store.sidebarContainment })
  })

  it('组满拒写（运行时硬上限兑底，菜单置灰为第一道防线）：满员组移回 no-op 不写存储', () => {
    const store = useSidebarOrderStore()
    store.applyMoveBackToSidebar('scheduled') // 记账出厂满员
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
    expect(store.sidebarContainment.bookkeeping).toEqual(['scheduled', 'merchants'])
  })

  it('非清单成员（在册主项、固定项）no-op 不写存储', () => {
    const store = useSidebarOrderStore()
    store.applyMoveBackToSidebar('reports')
    store.applyMoveBackToSidebar('dashboard' as ContainableViewName)
    expect(localStorage.getItem(ORDER_KEY)).toBeNull()
    expect(localStorage.getItem(CONTAINMENT_KEY)).toBeNull()
  })

  it('移回资产组成员：保单落资产组主项末位（出厂未满员，无需腾位）', () => {
    const store = useSidebarOrderStore()
    store.applyMoveBackToSidebar('policies')
    expect(store.sidebarContainment.assets).toEqual(['physicalAssets', 'insurers'])
    expect(store.sidebarGroupOrders.assets).toEqual(['investments', 'items', 'policies'])
  })

  it('移回组内最后一个收纳成员后清单为空（侧栏「更多」链接渲染条件失效）', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('reports')
    store.applyMoveIntoMore('search')
    store.applyMoveBackToSidebar('reports')
    store.applyMoveBackToSidebar('search')
    expect(store.sidebarContainment.insights).toEqual([])
  })

  it('移回的种子可再移入更多、可组内排序微调（主项词表对称，ADR-0063 决策 4 无例外清单）', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    store.applyMoveBackToSidebar('scheduled')
    store.applySidebarSort('scheduled', 'top')
    expect(store.sidebarGroupOrders.bookkeeping).toEqual(['scheduled', 'accounts', 'budget'])
    store.applyMoveIntoMore('scheduled')
    expect(store.sidebarGroupOrders.bookkeeping).toEqual(['accounts', 'budget'])
    expect(store.sidebarContainment.bookkeeping).toEqual(['merchants', 'transactions', 'scheduled'])
  })

  it('重启（新 pinia）后移回保持：种子不复活回收纳清单、主项保持（持久化往返）', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    store.applyMoveBackToSidebar('scheduled')
    reboot()
    const rebooted = useSidebarOrderStore()
    expect(rebooted.sidebarGroupOrders.bookkeeping).toEqual(['accounts', 'budget', 'scheduled'])
    expect(rebooted.sidebarContainment.bookkeeping).toEqual(['merchants', 'transactions'])
  })

  it('恢复默认排序复位移回：种子回收纳清单、主项回出厂（一键回出厂唯一通道不变）', () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('transactions')
    store.applyMoveBackToSidebar('scheduled')
    store.resetSidebarOrder()
    expect(store.sidebarGroupOrders.bookkeeping).toEqual(['transactions', 'accounts', 'budget'])
    expect(store.sidebarContainment.bookkeeping).toEqual(['scheduled', 'merchants'])
  })

  it('isSidebarMember（在册判定）：默认在册 = 七主项；移回种子后在册、移入后退出在册；固定项恒不在册', () => {
    const store = useSidebarOrderStore()
    expect(store.isSidebarMember('transactions')).toBe(true)
    expect(store.isSidebarMember('scheduled')).toBe(false)
    store.applyMoveIntoMore('transactions')
    expect(store.isSidebarMember('transactions')).toBe(false)
    store.applyMoveBackToSidebar('scheduled')
    expect(store.isSidebarMember('scheduled')).toBe(true)
    expect(store.isSidebarMember('dashboard')).toBe(false)
    expect(store.isSidebarMember('bogus')).toBe(false)
  })

  it('isViewContained（收纳在册判定）：出厂种子在册；移回后出册（/policies 路由守卫消费面）', () => {
    const store = useSidebarOrderStore()
    expect(store.isViewContained('policies')).toBe(true)
    store.applyMoveBackToSidebar('policies')
    expect(store.isViewContained('policies')).toBe(false)
    expect(store.isViewContained('physicalAssets')).toBe(true)
    expect(store.isViewContained('insurers')).toBe(true)
  })
})
