import { computed, h, onMounted, onUnmounted, ref } from 'vue'
import type { Router } from 'vue-router'
import type { DropdownOption } from 'naive-ui'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import { getSavedSidebarOrder, saveSidebarOrders, clearSidebarOrder, getSavedContainment, saveContainmentLists, clearContainment } from '@/utils/view-state'
import { t } from '@/i18n'

export interface ViewShortcut {
  /** 路由 name（与侧边栏菜单 key 一致） */
  name: string
  /** 主键：概览 '`'、各组固定键位带（'1'..'9'）、'0'（AI）或 ','（设置）；null = 无键位 */
  key: string | null
}

/**
 * 顺序源模块：侧边栏视图顺序单一来源（顺序 = 菜单顺序 = 组内键位）。
 * 侧栏为分组形态（issue #359 / ADR-0051）：组与组序固定（记账/资产/洞察），组内序可排
 * （右键菜单）且持久化对象收窄为组内序；分组标题不占键位、不参与排序与计数，
 * 键位按固定组带推导（ADR-0065，取代 ADR-0063 决策 2 的线性连续取键）——
 * 「位置即键位」收窄为组内带内：组内重排键位随动，跨组互不牵连。
 */

/**
 * 三组（域职责分组，组 id 即 i18n key `common.sidebarGroup.<id>`）。
 * 组与组序固定、成员闭集；各组主项 ≤3 是运行时硬上限（ADR-0063 决策 2），
 * 低频成员由各组「更多」收纳（GROUP_CONTAINMENT_SEEDS）：记账 = 定时、商户（#473），
 * 资产 = 保单、实物资产（#472 / #466），洞察出厂无收纳成员。
 * 键位注：键位只扫主项，按固定组带推导（ADR-0065，取代 ADR-0063 决策 2 线性推导）——
 * 出厂主项七项占 ⌘1–⌘5、⌘7、⌘8，⌘6 与 ⌘9 带内空置，组内补足后自然回填。
 */
export const SIDEBAR_GROUPS = [
  { id: 'bookkeeping', views: ['transactions', 'accounts', 'budget'] },
  { id: 'assets', views: ['investments', 'items'] },
  { id: 'insights', views: ['reports', 'search'] },
] as const

/**
 * 每组主项硬上限（issue #472/#475 / ADR-0063 决策 2，运行时不变量）：
 * 3 组 × 3 = ⌘1–⌘9 九键（ADR-0065），键位带封闭性的本体；组满时「移回侧栏」置灰（上限可见、可学习，不自动换出）。
 */
export const GROUP_MAIN_LIMIT = 3

export type SidebarGroupId = (typeof SIDEBAR_GROUPS)[number]['id']

/**
 * 固定项词表：概览首位（与启动落地页一致）、AI 倒数第二、设置末位。
 * 全局「更多」固定项已退役（issue #473 / ADR-0063 决策 1/5），不再入线性序——
 * 旧路由与旧视图名由路由表重定向记录承接（见 router 的 /more 记录）。
 */
export const FIRST_VIEW = 'dashboard'
export const PENULTIMATE_VIEW = 'ai'
export const LAST_VIEW = 'settings'

/** 线性默认序（出厂快照）：概览 + 各组按组序展开 + AI + 设置 */
export const DEFAULT_VIEW_ORDER = [
  FIRST_VIEW,
  ...SIDEBAR_GROUPS.flatMap((g) => g.views),
  PENULTIMATE_VIEW,
  LAST_VIEW,
] as const

export type ViewName = (typeof DEFAULT_VIEW_ORDER)[number]

/** 主项词表（组内可排区）：各组成员按组序展开，相对顺序即默认相对顺序。
 *  每组 ≤3 硬上限保证键位带不溢出（ADR-0063 决策 2）；收纳成员不入本表、无键位。
 *  保留字面量元组类型（不加宽到 ViewName）：主项集合是 ContainableViewName 词表的组成部分。 */
export const ARRANGEABLE_VIEWS = SIDEBAR_GROUPS.flatMap((g) => [...g.views])

/** 固定项例外判定：主项（可排区）为真，概览/AI/设置三固定项为假（右键无菜单）。 */
export function isArrangeableView(v: unknown): v is ViewName {
  return typeof v === 'string' && (ARRANGEABLE_VIEWS as readonly string[]).includes(v)
}

/**
 * 视图 → 所属组（主项各有其组；概览/AI/设置与未知名不在任何组，返回 null）。
 * 出厂收纳种子词法归属出厂组（issue #475 / ADR-0063 决策 4 不跨组）：移回侧栏后即以
 * 该组主项身份在册，排序/移入写路径凭此找到组；种子仍在清单时调用方各有在册守卫
 * （prev.includes / 清单 includes），词法归属不构成误伤。
 */
export function groupOfView(name: AnyViewName): SidebarGroupId | null {
  for (const g of SIDEBAR_GROUPS) {
    if ((g.views as readonly string[]).includes(name)) return g.id
  }
  for (const g of SIDEBAR_GROUPS) {
    if ((GROUP_CONTAINMENT_SEEDS[g.id] as readonly string[]).includes(name)) return g.id
  }
  return null
}

export type SidebarGroupOrders = Readonly<Record<SidebarGroupId, readonly ContainableViewName[]>>

function defaultGroupOrders(): Record<SidebarGroupId, ContainableViewName[]> {
  const result = {} as Record<SidebarGroupId, ContainableViewName[]>
  for (const g of SIDEBAR_GROUPS) result[g.id] = [...g.views]
  return result
}

/**
 * 组内序解析（纯函数）：已存「组 id → 视图名数组」→ 各组解析后的组内序。
 * 整体形状防御：仅接受普通对象——旧平铺数组（issue #270 形态）、null、标量等
 * 非对象输入整体回退默认序（旧平铺数据不迁移，直接回出厂序）。
 * 组内防御：非法名过滤（含固定项名、他组成员、未知名、非字符串项）→ 去重（保留首现）
 * → 缺失项按默认序补入组内末尾；组值非数组则该组整体回退默认序（他组不受牵连）。
 * 第二参数 `contained`（issue #474）：已解析的各组收纳清单——清单成员不再视作
 * 「缺失主项」回填组内序（清单是收纳成员资格的唯一事实源，主项不复活）。
 */
export function parseGroupOrders(
  raw: unknown,
  contained: Readonly<Record<SidebarGroupId, readonly string[]>> = defaultContainmentLists(),
): Record<SidebarGroupId, ContainableViewName[]> {
  const result = defaultGroupOrders()
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return result
  const rec = raw as Record<string, unknown>
  for (const g of SIDEBAR_GROUPS) {
    const excluded = contained[g.id] ?? []
    // 合法主项池 = 出厂主项 ∪ 出厂种子（issue #475：移回的种子以主项身份在册）；
    // 仍在清单中的种子被 excluded 排除（清单是收纳成员资格的唯一事实源，#474 语义不变）。
    const members = [...(g.views as readonly string[]), ...(GROUP_CONTAINMENT_SEEDS[g.id] as readonly string[])]
      .filter((v) => !excluded.includes(v))
    const kept: ContainableViewName[] = []
    const seen = new Set<string>()
    if (Array.isArray(rec[g.id])) {
      for (const item of rec[g.id] as unknown[]) {
        if (typeof item !== 'string' || !members.includes(item) || seen.has(item)) continue
        seen.add(item)
        kept.push(item as ContainableViewName)
      }
    }
    for (const name of members) {
      if (!seen.has(name)) kept.push(name as ContainableViewName)
    }
    result[g.id] = kept
  }
  return result
}

// ---------------------------------------------------------------------------
// 组内收纳清单（issue #472 / ADR-0063 决策 3/5）：每组一个有序收纳清单，
// 成员资格与页签顺序同源——清单序 = 该组「更多」页页签序，入 ViewState 跨启动持久化。
// 与组内序同族同机制：出厂种子（开发者清单转任）+ 同型解析防御。
// 资产组出厂成员 = 保单（#472）+ 实物资产（#466 / ADR-0064，随域归位）；
// 记账组出厂成员 = 定时、商户（#473：定时自主项迁入，商户自全局「更多」迁入）；
// 洞察种子为空、出厂不渲染「更多」链接（路由预建）；用户移入由 #474 落地、移回由 #475。
// 数据块先于组内序状态：启动读路径先解析收纳清单，组内序解析凭它排除收纳成员
// （issue #474：移入后主项退出组内序，重启解析不得因「缺失补尾」复活）。
// ---------------------------------------------------------------------------

/**
 * 每组收纳清单出厂种子（开发者清单，ADR-0063 决策 3）：
 * 记账 = [定时, 商户]（页签序 = 清单序，#473）；
 * 资产 = [保单, 实物资产]（#472/#466，追加在后，ADR-0055 决策 2 追加先例）；洞察 = 空。
 */
export const GROUP_CONTAINMENT_SEEDS = {
  bookkeeping: ['scheduled', 'merchants'],
  assets: ['policies', 'physicalAssets'],
  insights: [],
} as const satisfies Record<SidebarGroupId, readonly string[]>

/** 收纳视图名 = 出厂种子成员 + 任一本组主项（issue #474 移入自由，ADR-0063 决策 4）；固定项不可收纳，不在词表。 */
export type ContainableViewName =
  | (typeof GROUP_CONTAINMENT_SEEDS)[SidebarGroupId][number]
  | (typeof ARRANGEABLE_VIEWS)[number]

/** 每组收纳清单（只读形状）：与 SidebarGroupOrders 同族。 */
export type SidebarContainmentLists = Readonly<Record<SidebarGroupId, readonly ContainableViewName[]>>

/**
 * 全部已知名词表（issue #475）：固定项 ∪ 主项 ∪ 出厂种子。
 * 写路径以此入参、运行时在册守卫收窄（非成员 no-op），调用方（菜单/页签）天然只产合法名。
 */
export type AnyViewName = ViewName | ContainableViewName

function defaultContainmentLists(): Record<SidebarGroupId, ContainableViewName[]> {
  const result = {} as Record<SidebarGroupId, ContainableViewName[]>
  for (const g of SIDEBAR_GROUPS) result[g.id] = [...GROUP_CONTAINMENT_SEEDS[g.id]]
  return result
}

/**
 * 收纳清单解析（纯函数，与 parseGroupOrders 同型防御）：
 * 已存「组 id → 收纳视图名数组」→ 各组解析后的收纳清单。
 * 整体形状防御：仅接受普通对象——null、数组、标量等非对象输入整体回出厂种子。
 * 组内防御：合法成员 = 出厂种子 + 本组主项（issue #474 移入自由，ADR-0063 决策 4——
 * 任一主项可入本组「更多」；合法集在同一组迭代内就地取词表，不另设可错位的注册表；
 * 他组成员、他组主项、固定项名、未知名、非字符串项一律非法——不跨组）
 * → 去重（保留首现）→ 缺失出厂成员按出厂序补尾；组值非数组则该组整体回种子（他组不受牵连）。
 * 第二参数 `rawOrders`（issue #475）：未解析的组内序原始值——「移回豁免」判定面：
 * 清单数组存在且未列某种子、而该种子已在本组组内序数组中 = 用户已把它移回侧栏（主项），
 * 不按「缺失补尾」复活回收纳清单。清单存储缺失（旧版/出厂）不豁免：#473 迁移语义
 * （存量组内序含种子判收纳）与出厂种子补尾不受影响。
 */
export function parseContainmentLists(raw: unknown, rawOrders: unknown = null): Record<SidebarGroupId, ContainableViewName[]> {
  const result = defaultContainmentLists()
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return result
  const rec = raw as Record<string, unknown>
  const orderRec =
    typeof rawOrders === 'object' && rawOrders !== null && !Array.isArray(rawOrders)
      ? (rawOrders as Record<string, unknown>)
      : null
  for (const g of SIDEBAR_GROUPS) {
    const legal = [...GROUP_CONTAINMENT_SEEDS[g.id], ...(g.views as readonly string[])]
    const kept: ContainableViewName[] = []
    const seen = new Set<string>()
    const rawArr = Array.isArray(rec[g.id]) ? (rec[g.id] as unknown[]) : null
    if (rawArr) {
      for (const item of rawArr) {
        if (typeof item !== 'string' || !legal.includes(item) || seen.has(item)) continue
        seen.add(item)
        kept.push(item as ContainableViewName)
      }
    }
    const orderArr = orderRec && Array.isArray(orderRec[g.id]) ? (orderRec[g.id] as unknown[]) : null
    for (const name of GROUP_CONTAINMENT_SEEDS[g.id]) {
      if (seen.has(name)) continue
      // 移回豁免（issue #475，判定面见函数注释）：清单在且未列、组内序在——已是主项
      if (rawArr && orderArr?.includes(name)) continue
      kept.push(name as ContainableViewName)
    }
    result[g.id] = kept
  }
  return result
}

/**
 * 已存收纳清单：启动读路径（解析防御后回出厂种子；第二参传原始组内序供移回豁免判定，#475）。
 * 写路径收口在 applyMoveIntoMore（#474 移入）/ applyMoveBackToSidebar（#475 移回）/
 * resetSidebarOrder（复位）：点选即写、重启保持。
 */
const savedSidebarOrderRaw = getSavedSidebarOrder()
const containmentLists = ref<SidebarContainmentLists>(parseContainmentLists(getSavedContainment(), savedSidebarOrderRaw))

/**
 * 已存组内序：启动读路径（解析防御后回退默认序）。
 * 写路径收口在 applySidebarSort（issue #270/#359 排序）/ applyMoveIntoMore（#474 移入）/
 * applyMoveBackToSidebar（#475 移回）/ resetSidebarOrder（复位）：四者同步更新此 ref 与
 * 持久化，菜单顺序与键位经 viewShortcuts 自动随动。
 */
const groupOrders = ref<Record<SidebarGroupId, ContainableViewName[]>>(
  parseGroupOrders(savedSidebarOrderRaw, containmentLists.value),
)

/** 当前组内序（只读响应式）：侧栏排序菜单构建等消费；写路径不经它。 */
export const sidebarGroupOrders = computed<SidebarGroupOrders>(() => groupOrders.value)

/** 当前每组收纳清单（只读响应式）：组内「更多」页页签与侧栏链接显隐消费。 */
export const sidebarContainment = computed<SidebarContainmentLists>(() => containmentLists.value)

/** 渲染用分组（组序固定 + 组内当前序，含移回的种子成员）：侧栏菜单构建消费。 */
export const sidebarGroups = computed<readonly { id: SidebarGroupId; views: readonly ContainableViewName[] }[]>(
  () => SIDEBAR_GROUPS.map((g) => ({ id: g.id, views: groupOrders.value[g.id] })),
)

// ---------------------------------------------------------------------------
// 右键「移入更多」（issue #474 / ADR-0063 决策 4）：移入纯函数 + 写路径。
// 移入自由、无例外面：任一主项可入本组「更多」，追加清单尾 = 该组「更多」页最后一个页签；
// 空种子组（洞察）首个移入即清单非空，侧栏「更多」链接渲染条件即刻满足。
// 不跨组：groupOfView 把写路径钉在本组，跨组移动结构上不可达。
// ---------------------------------------------------------------------------

/**
 * 移入纯函数：主项名追加为本组收纳清单尾，返回新清单对象（不改输入）。
 * 清单序 = 页签序——移入即该组「更多」页最后一个页签；
 * 已在清单中的成员内容不变（去重保位，no-op 语义）。
 */
export function moveIntoContainment(
  lists: SidebarContainmentLists,
  gid: SidebarGroupId,
  name: ContainableViewName,
): SidebarContainmentLists {
  const list = lists[gid]
  if (list.includes(name)) return lists
  return { ...lists, [gid]: [...list, name] }
}

/** 右键「移入更多」：主项退出组内序（键位随动重排）+ 追加本组收纳清单尾；
 *  点选即写（组内序与收纳清单双存储同步持久化，与排序写路径同一纪律）；
 *  固定项/收纳成员/未知名 no-op 不写存储（保住「恢复默认 = 删 key」语义）。
 *  入参 AnyViewName（issue #475）：移回侧栏的种子以主项身份在册后，可再右键移入。 */
export function applyMoveIntoMore(name: AnyViewName) {
  const gid = groupOfView(name)
  if (!gid) return
  const prev = groupOrders.value[gid]
  if (!(prev as readonly string[]).includes(name)) return
  groupOrders.value = { ...groupOrders.value, [gid]: prev.filter((v) => v !== name) }
  // prev.includes 已证明 name 是本组在册主项（运行时守卫对应 ContainableViewName 词表）
  containmentLists.value = moveIntoContainment(containmentLists.value, gid, name as ContainableViewName)
  saveSidebarOrders(groupOrders.value)
  saveContainmentLists(containmentLists.value)
}

// ---------------------------------------------------------------------------
// 右键「移回侧栏」（issue #475 / ADR-0063 决策 2/4）：组满置灰判定 + 移回纯函数 +
// 页签右键菜单构建 + 写路径。移回自由度受 ≤3 硬上限约束：组满 3 主项时菜单置灰 +
// 提示（上限可见、可学习），不自动换出、不弹窗腾位；写路径同判定兑底拒写。
// 移回 = 清单删除 + 落本组主项末位（可再右键排序微调）；主项集变化后键位重排；
// 移回组内最后一个收纳成员后清单为空，侧栏「更多」链接随之消失（渲染条件失效）。
// ---------------------------------------------------------------------------

/**
 * 组满判定纯函数：本组主项数已达硬上限——「移回侧栏」菜单置灰与写路径拒写的同一判定面
 * （ADR-0063 决策 2：≤3 是运行时不变量，不是出厂布局建议）。
 */
export function isGroupFull(order: readonly ContainableViewName[]): boolean {
  return order.length >= GROUP_MAIN_LIMIT
}

/**
 * 移回纯函数：从本组收纳清单删除成员，返回新清单对象（不改输入）。
 * 清单序 = 页签序——移回即该页签从「更多」页消失；成员不在清单时原样返回内容（no-op 语义）。
 */
export function moveBackToSidebar(
  lists: SidebarContainmentLists,
  gid: SidebarGroupId,
  name: ContainableViewName,
): SidebarContainmentLists {
  const list = lists[gid]
  if (!list.includes(name)) return lists
  return { ...lists, [gid]: list.filter((v) => v !== name) }
}

/**
 * 「更多」页页签右键菜单构建纯函数：单菜单项「移回侧栏」。
 * 组满时置灰且不隐藏菜单项（上限可见、可学习），提示文案（组满须先移出一个主项）
 * 经 i18n 挂在标签渲染函数里（zh-CN 与 en-US 同步）。
 */
export function buildTabContextMenuOptions(order: readonly ContainableViewName[]): DropdownOption[] {
  const full = isGroupFull(order)
  const label: DropdownOption['label'] = full
    ? () =>
        h('div', null, [
          h('div', null, t('common.sidebarContainment.backToSidebar')),
          h('div', { style: 'font-size:12px;opacity:.6;white-space:normal;max-width:220px' }, t('common.sidebarContainment.backBlockedFull')),
        ])
    : t('common.sidebarContainment.backToSidebar')
  return [{ key: 'backToSidebar', disabled: full, label }]
}

/** 右键「移回侧栏」：成员退出本组收纳清单 + 落本组主项末位（键位随动重排）；
 *  点选即写（收纳清单与组内序双存储同步持久化，与移入写路径同一纪律）；
 *  组满拒写（≤3 运行时硬上限兑底，菜单置灰为第一道防线）；
 *  非清单成员（在册主项/固定项/未知名）no-op 不写存储。 */
export function applyMoveBackToSidebar(name: AnyViewName) {
  const gid = groupOfView(name)
  if (!gid) return
  const list = containmentLists.value[gid]
  if (!(list as readonly string[]).includes(name)) return
  const order = groupOrders.value[gid]
  if (isGroupFull(order)) return
  // list.includes 已证明 name 是本组收纳成员（运行时守卫对应 ContainableViewName 词表）
  const member = name as ContainableViewName
  containmentLists.value = moveBackToSidebar(containmentLists.value, gid, member)
  groupOrders.value = { ...groupOrders.value, [gid]: [...order, member] }
  saveContainmentLists(containmentLists.value)
  saveSidebarOrders(groupOrders.value)
}

/** 侧栏在册判定（运行时）：名下在任一组当前组内序中（含移回的种子成员，#475）。
 *  侧栏右键菜单按此附事件——在册成员（含移回种子）可排序/移入，固定项与
 *  仍在清单的收纳成员不可（词法 isArrangeableView 不再看运行时移回态，改用本判定）。 */
export function isSidebarMember(v: unknown): v is ContainableViewName {
  if (typeof v !== 'string') return false
  return SIDEBAR_GROUPS.some((g) => (groupOrders.value[g.id] as readonly string[]).includes(v))
}

/** 收纳在册判定（运行时）：名在任一组当前收纳清单中（#475 路由守卫消费：/policies 分流）。 */
export function isViewContained(name: ContainableViewName): boolean {
  return SIDEBAR_GROUPS.some((g) => (containmentLists.value[g.id] as readonly string[]).includes(name))
}

// ---------------------------------------------------------------------------
// 组内自定义排序（issue #270，#359 收窄为组内）：右键菜单 → 移动纯函数 + 菜单构建
// 纯函数 + 写路径。组内序是唯一事实源（groupOrders），移动只见一张组内数组——
// 组与组序固定，跨组移动结构上不可达；点选即重排并立即持久化，
// 菜单顺序与键位经 viewShortcuts 自动随动。
// ---------------------------------------------------------------------------

/** 排序动作：组内上移一位 / 下移一位 / 移到组内顶部 / 移到组内底部 */
export type SidebarSortAction = 'up' | 'down' | 'top' | 'bottom'

/**
 * 移动纯函数：在给定组内序中把 name 移动到目标位置，返回新数组（不改输入）。
 * 已在对应边界（组内首位上移/移顶、组内末位下移/移底）时内容不变；
 * name 不在序中（如固定项名、他组成员）原样返回内容。
 */
export function moveArrangeable(
  order: readonly ContainableViewName[],
  name: AnyViewName,
  action: SidebarSortAction,
): ContainableViewName[] {
  const index = (order as readonly string[]).indexOf(name)
  if (index === -1) return [...order]
  const last = order.length - 1
  const target = action === 'up' ? index - 1
    : action === 'down' ? index + 1
    : action === 'top' ? 0
    : last
  if (target < 0 || target > last || target === index) return [...order]
  const next = [...order]
  next.splice(index, 1)
  // index 有效已证明 name ∈ order（元素类型 ContainableViewName）
  next.splice(target, 0, name as ContainableViewName)
  return next
}

/**
 * 排序菜单 key 与移动动作同一词表（key 即 action，杜绝两套词表错位）：
 * up / down / top / bottom 四种移动 + reset 恢复默认排序。
 * 「移入更多」（intoMore）不是排序动作，由调用方按菜单 key 分派（见 App.vue）。
 */
export function isSidebarSortAction(v: string): v is SidebarSortAction {
  return v === 'up' || v === 'down' || v === 'top' || v === 'bottom'
}

/**
 * 排序菜单选项构建纯函数（含组内边界置灰）：
 * 上移一位 / 下移一位 / 移到组内顶部 / 移到组内底部 /
 * 分隔线 / 移入更多（issue #474：排序动作之后、分隔线隔开，移入自由恒可用）/
 * 分隔线 / 恢复默认排序（恒可用）。
 * 置灰按传入组内序的边界判定：首位上移、移顶置灰；末位下移、移底置灰。
 */
export function buildSidebarSortMenuOptions(
  name: ContainableViewName,
  order: readonly ContainableViewName[],
): DropdownOption[] {
  const index = order.indexOf(name)
  const atTop = index <= 0
  const atBottom = index === order.length - 1
  return [
    { label: t('common.sidebarSort.up'), key: 'up', disabled: atTop },
    { label: t('common.sidebarSort.down'), key: 'down', disabled: atBottom },
    { label: t('common.sidebarSort.top'), key: 'top', disabled: atTop },
    { label: t('common.sidebarSort.bottom'), key: 'bottom', disabled: atBottom },
    { type: 'divider', key: 'sort-divider' },
    { label: t('common.sidebarContainment.intoMore'), key: 'intoMore', disabled: false },
    { type: 'divider', key: 'reset-divider' },
    { label: t('common.sidebarSort.reset'), key: 'reset', disabled: false },
  ]
}

/** 点选即重排并立即持久化（写路径唯一出处；顺序变更经 viewShortcuts 自动随动）。
 *  只在 name 所属组内移动（组与组序固定，跨组移动不可达）；
 *  边界 no-op 不写存储：保住「恢复默认 = 删除 key」语义，出厂默认序将来调整时自动跟随。 */
export function applySidebarSort(name: AnyViewName, action: SidebarSortAction) {
  const gid = groupOfView(name)
  if (!gid) return
  const prev = groupOrders.value[gid]
  const next = moveArrangeable(prev, name, action)
  if (prev.every((v, i) => v === next[i])) return
  groupOrders.value = { ...groupOrders.value, [gid]: next }
  saveSidebarOrders(groupOrders.value)
}

/** 恢复默认排序：组内序与收纳清单一并清除回出厂（ADR-0063 决策 4，一键回出厂布局的唯一通道）。 */
export function resetSidebarOrder() {
  clearSidebarOrder()
  clearContainment()
  groupOrders.value = defaultGroupOrders()
  containmentLists.value = defaultContainmentLists()
}

// ---------------------------------------------------------------------------
// 键位按固定组带推导（ADR-0065，取代 ADR-0063 决策 2 的线性连续取键）：
// 概览恒 '`'（运行时验证前提见 ADR-0065 决策 1）；三组按固定组序各占三键带
// ——记账 1–3、资产 4–6、洞察 7–9，组内序定带内位置；AI 恒 '0'；设置为唯一例外用 ','。
// 收纳成员与「更多」链接不在推导表内——无键位、不出提示、不可键盘触发。
// 组主项不足 3 时带内空键保留（出厂 ⌘6、⌘9 空置），不跨组压缩；
// 1–9 九键对 3×3 理论上限，键位封闭严格成立；组内重排键位带内随动。
// ---------------------------------------------------------------------------

const LEAD_KEY = '`'
/** 每组固定键位带（组序固定，带序随组序）：记账 1–3、资产 4–6、洞察 7–9（ADR-0065） */
const GROUP_KEY_BANDS: Record<SidebarGroupId, readonly string[]> = {
  bookkeeping: ['1', '2', '3'],
  assets: ['4', '5', '6'],
  insights: ['7', '8', '9'],
}
const PENULTIMATE_KEY = '0'
const LAST_KEY = ','

/**
 * 键位推导纯函数：由组内序按固定组带派生全部视图快捷键（组内序定带内位置，重排即带内重排键位）。
 * 只扫主项：组主项不足 3 时带尾空键保留（不产记录）；超位（理论上限内不发生）得 null。
 */
export function deriveViewShortcuts(orders: SidebarGroupOrders): ViewShortcut[] {
  const shortcuts: ViewShortcut[] = [{ name: FIRST_VIEW, key: LEAD_KEY }]
  for (const g of SIDEBAR_GROUPS) {
    const band = GROUP_KEY_BANDS[g.id]
    orders[g.id].forEach((name, i) => {
      shortcuts.push({ name, key: band[i] ?? null })
    })
  }
  shortcuts.push({ name: PENULTIMATE_VIEW, key: PENULTIMATE_KEY })
  shortcuts.push({ name: LAST_VIEW, key: LAST_KEY })
  return shortcuts
}

/**
 * 视图快捷键映射（响应式）：由组内序按固定组带推导，键随组内位置（组内重排侧栏即带内重排键位）。
 * 每个侧栏主项/固定项恰一条记录：概览 ⌘`、主项按所在组键位带取 ⌘1–⌘9、AI ⌘0、
 * 设置 Cmd/Ctrl+,（macOS「设置」惯例键位，避免占用 Cmd+S 的「保存」肌肉记忆）；
 * 收纳成员与「更多」链接不入表（无键位、不出提示、不可键盘触发）。
 */
export const viewShortcuts = computed<ViewShortcut[]>(() => deriveViewShortcuts(groupOrders.value))

/** macOS 用 Cmd（metaKey），Windows/Linux 用 Ctrl（ctrlKey） */
export function isMacPlatform(): boolean {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const platform = nav.userAgentData?.platform ?? navigator.platform
  return /mac/i.test(platform)
}

/** 菜单提示文案：macOS 显示 ⌘1，其余显示 ⌃1（⌃ 为 Control 键符，与 ⌘ 同族单字形、跨平台等宽对齐） */
export function shortcutHint(key: string): string {
  return isMacPlatform() ? `⌘${key}` : `⌃${key}`
}

/** 是否恰按主修饰键（不混按 Ctrl/Cmd 双键） */
function isPrimaryModifier(e: KeyboardEvent): boolean {
  return isMacPlatform() ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
}

/** 纯函数：命中视图快捷键则返回路由 name，否则 null（排除 Shift/Alt/混按/未映射键/无键位项） */
export function matchViewShortcut(e: KeyboardEvent): string | null {
  if (e.altKey || e.shiftKey) return null
  if (!isPrimaryModifier(e)) return null
  return viewShortcuts.value.find((s) => s.key !== null && e.key === s.key)?.name ?? null
}

/** 覆盖层（弹窗/确认框/下拉菜单/日历面板）打开时抑制快捷键，避免在编辑/确认/选择中途触发。
 *  状态来自弹层注册表（ADR-0035）——弹层封装组件（AppModal/AppSelect/AppDatePicker/
 *  AppDropdown/AppPopconfirm/useAppDialog）显式上报开/关，不做 DOM 推断。 */
export { hasOpenOverlay }

/** 注册全局 keydown 监听：命中视图快捷键时切换路由 */
export function useViewShortcuts(router: Router) {
  const onKeydown = (e: KeyboardEvent) => {
    const name = matchViewShortcut(e)
    if (!name) return
    if (hasOpenOverlay()) return
    e.preventDefault()
    if (router.currentRoute.value.name !== name) {
      router.push({ name })
    }
  }
  onMounted(() => window.addEventListener('keydown', onKeydown))
  onUnmounted(() => window.removeEventListener('keydown', onKeydown))
}
