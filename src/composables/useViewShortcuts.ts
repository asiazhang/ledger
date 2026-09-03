import { computed, onMounted, onUnmounted, ref } from 'vue'
import type { Router } from 'vue-router'
import type { DropdownOption } from 'naive-ui'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import { getSavedSidebarOrder, saveSidebarOrders, clearSidebarOrder, getSavedContainment, clearContainment } from '@/utils/view-state'
import { t } from '@/i18n'

export interface ViewShortcut {
  /** 路由 name（与侧边栏菜单 key 一致） */
  name: string
  /** 主键：按线性位置推导的 '1'..'9'、'0'（AI）或 ','（设置）；null = 无键位 */
  key: string | null
}

/** 固定项 4：「更多」聚合视图（issue #372）——洞察组之后、AI 之前的第四固定项。 */
export const EXTRA_VIEW = 'more'

/**
 * 顺序源模块：侧边栏视图顺序单一来源（顺序 = 菜单顺序 = 数字键位）。
 * 侧栏为分组形态（issue #359 / ADR-0051）：组与组序固定（记账/资产/洞察），组内序可排
 * （右键菜单）且持久化对象收窄为组内序；分组标题不占键位、不参与排序与计数，
 * 键位按线性位置推导——「位置即键位」哲学在数字键物理上限内的诚实延伸。
 */

/**
 * 三组（域职责分组，组 id 即 i18n key `common.sidebarGroup.<id>`）。
 * 组与组序固定、成员闭集；「资产」组 = 投资（金融资产）、物品（实物资产）——
 * 低频的保单已迁入「更多」聚合视图（issue #371/#372），可排区收窄为八项，
 * 数字键位恰好十键十视图全占、无死角。
 */
export const SIDEBAR_GROUPS = [
  { id: 'bookkeeping', views: ['transactions', 'accounts', 'budget', 'scheduled'] },
  { id: 'assets', views: ['investments', 'items'] },
  { id: 'insights', views: ['reports', 'search'] },
] as const

export type SidebarGroupId = (typeof SIDEBAR_GROUPS)[number]['id']

/**
 * 固定项词表：概览首位（与启动落地页一致）、「更多」（洞察组之后、AI 之前，
 * 不参与组内排序、不占数字键位）、AI 倒数第二、设置末位。
 */
export const FIRST_VIEW = 'dashboard'
export const PENULTIMATE_VIEW = 'ai'
export const LAST_VIEW = 'settings'

/** 线性默认序（出厂快照）：概览 + 各组按组序展开 + 更多 + AI + 设置 */
export const DEFAULT_VIEW_ORDER = [
  FIRST_VIEW,
  ...SIDEBAR_GROUPS.flatMap((g) => g.views),
  EXTRA_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
] as const

export type ViewName = (typeof DEFAULT_VIEW_ORDER)[number]

/** 可排区（组内）：各组成员按组序展开，相对顺序即默认相对顺序。
 *  容量八项 = 数字键位带 2..9 恰好覆盖（issue #372：十键十视图全占，
 *  「可排区末位无键位」wart 消除）。 */
export const ARRANGEABLE_VIEWS: readonly ViewName[] = SIDEBAR_GROUPS.flatMap((g) => [...g.views])

/** 固定项例外判定：可排区八项为真，概览/更多/AI/设置四固定项为假（右键无菜单）。 */
export function isArrangeableView(v: unknown): v is ViewName {
  return typeof v === 'string' && (ARRANGEABLE_VIEWS as readonly string[]).includes(v)
}

/** 视图 → 所属组（可排区八项各有其组；概览/更多/AI/设置与未知名不在任何组，返回 null）。 */
export function groupOfView(name: ViewName): SidebarGroupId | null {
  for (const g of SIDEBAR_GROUPS) {
    if ((g.views as readonly string[]).includes(name)) return g.id
  }
  return null
}

export type SidebarGroupOrders = Readonly<Record<SidebarGroupId, readonly ViewName[]>>

function defaultGroupOrders(): Record<SidebarGroupId, ViewName[]> {
  const result = {} as Record<SidebarGroupId, ViewName[]>
  for (const g of SIDEBAR_GROUPS) result[g.id] = [...g.views]
  return result
}

/**
 * 组内序解析（纯函数）：已存「组 id → 视图名数组」→ 各组解析后的组内序。
 * 整体形状防御：仅接受普通对象——旧平铺数组（issue #270 形态）、null、标量等
 * 非对象输入整体回退默认序（旧平铺数据不迁移，直接回出厂序）。
 * 组内防御：非法名过滤（含固定项名、他组成员、未知名、非字符串项）→ 去重（保留首现）
 * → 缺失项按默认序补入组内末尾；组值非数组则该组整体回退默认序（他组不受牵连）。
 */
export function parseGroupOrders(raw: unknown): Record<SidebarGroupId, ViewName[]> {
  const result = defaultGroupOrders()
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return result
  const rec = raw as Record<string, unknown>
  for (const g of SIDEBAR_GROUPS) {
    const members = g.views as readonly string[]
    const kept: ViewName[] = []
    const seen = new Set<string>()
    if (Array.isArray(rec[g.id])) {
      for (const item of rec[g.id] as unknown[]) {
        if (typeof item !== 'string' || !members.includes(item) || seen.has(item)) continue
        seen.add(item)
        kept.push(item as ViewName)
      }
    }
    for (const name of members) {
      if (!seen.has(name)) kept.push(name as ViewName)
    }
    result[g.id] = kept
  }
  return result
}

/**
 * 已存组内序：启动读路径（解析防御后回退默认序）。
 * 写路径收口在 applySidebarSort / resetSidebarOrder（issue #270/#359）：两者同步更新此 ref
 * 与持久化，菜单顺序与键位经 viewShortcuts 自动随动。
 */
const groupOrders = ref<Record<SidebarGroupId, ViewName[]>>(parseGroupOrders(getSavedSidebarOrder()))

/** 当前组内序（只读响应式）：侧栏排序菜单构建等消费；写路径不经它。 */
export const sidebarGroupOrders = computed<SidebarGroupOrders>(() => groupOrders.value)

// ---------------------------------------------------------------------------
// 组内收纳清单（issue #472 / ADR-0063 决策 3/5）：每组一个有序收纳清单，
// 成员资格与页签顺序同源——清单序 = 该组「更多」页页签序，入 ViewState 跨启动持久化。
// 与组内序同族同机制：出厂种子（开发者清单转任）+ 同型解析防御。
// 本票（#472）仅资产组启用（保单迁入），记账/洞察种子为空、不渲染「更多」链接；
// 记账（定时、商户）与用户移入/移回由后续票落地，届时扩展种子与合法成员注册表。
// ---------------------------------------------------------------------------

/**
 * 每组收纳清单出厂种子（开发者清单，ADR-0063 决策 3）：
 * 资产 = [保单]；记账、洞察 = 空。后续票扩展：记账 = [定时, 商户]（#473）。
 */
export const GROUP_CONTAINMENT_SEEDS = {
  bookkeeping: [],
  assets: ['policies'],
  insights: [],
} as const satisfies Record<SidebarGroupId, readonly string[]>

/** 收纳视图名（词表随出厂种子与移入成员扩展；本票仅保单） */
export type ContainableViewName = (typeof GROUP_CONTAINMENT_SEEDS)[SidebarGroupId][number]

/** 每组收纳清单（只读形状）：与 SidebarGroupOrders 同族。 */
export type SidebarContainmentLists = Readonly<Record<SidebarGroupId, readonly ContainableViewName[]>>

function defaultContainmentLists(): Record<SidebarGroupId, ContainableViewName[]> {
  const result = {} as Record<SidebarGroupId, ContainableViewName[]>
  for (const g of SIDEBAR_GROUPS) result[g.id] = [...GROUP_CONTAINMENT_SEEDS[g.id]]
  return result
}

/**
 * 各组合法收纳成员注册表（解析防御用）：本票 = 出厂种子；
 * 用户移入（#474）落地后并入本组主项——注册表与种子在此分离，补尾仍按出厂序。
 */
const CONTAINABLE_VIEWS: Record<SidebarGroupId, readonly string[]> = GROUP_CONTAINMENT_SEEDS

/**
 * 收纳清单解析（纯函数，与 parseGroupOrders 同型防御）：
 * 已存「组 id → 收纳视图名数组」→ 各组解析后的收纳清单。
 * 整体形状防御：仅接受普通对象——null、数组、标量等非对象输入整体回出厂种子。
 * 组内防御：非法名过滤（主项名、他组成员、固定项名、未知名、非字符串项——不跨组）
 * → 去重（保留首现）→ 缺失出厂成员按出厂序补尾；组值非数组则该组整体回种子（他组不受牵连）。
 * 注：本票合法成员注册表 = 出厂种子，用户移入落地后合法集超出种子，清单序才开始偏离种子序。
 */
export function parseContainmentLists(raw: unknown): Record<SidebarGroupId, ContainableViewName[]> {
  const result = defaultContainmentLists()
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) return result
  const rec = raw as Record<string, unknown>
  for (const g of SIDEBAR_GROUPS) {
    const legal = CONTAINABLE_VIEWS[g.id]
    const kept: ContainableViewName[] = []
    const seen = new Set<string>()
    if (Array.isArray(rec[g.id])) {
      for (const item of rec[g.id] as unknown[]) {
        if (typeof item !== 'string' || !legal.includes(item) || seen.has(item)) continue
        seen.add(item)
        kept.push(item as ContainableViewName)
      }
    }
    for (const name of GROUP_CONTAINMENT_SEEDS[g.id]) {
      if (!seen.has(name)) kept.push(name as ContainableViewName)
    }
    result[g.id] = kept
  }
  return result
}

/**
 * 已存收纳清单：启动读路径（解析防御后回出厂种子）。
 * 本票（#472）无用户移入/移回，写路径仅「恢复默认排序」复位（resetSidebarOrder）；
 * 移入/移回写路径（saveContainmentLists 点选即写）由后续票接入。
 */
const containmentLists = ref<Record<SidebarGroupId, ContainableViewName[]>>(
  parseContainmentLists(getSavedContainment()),
)

/** 当前每组收纳清单（只读响应式）：组内「更多」页页签与侧栏链接显隐消费。 */
export const sidebarContainment = computed<SidebarContainmentLists>(() => containmentLists.value)

/** 渲染用分组（组序固定 + 组内当前序）：侧栏菜单构建消费。 */
export const sidebarGroups = computed<readonly { id: SidebarGroupId; views: readonly ViewName[] }[]>(
  () => SIDEBAR_GROUPS.map((g) => ({ id: g.id, views: groupOrders.value[g.id] })),
)

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
  order: readonly ViewName[],
  name: ViewName,
  action: SidebarSortAction,
): ViewName[] {
  const index = order.indexOf(name)
  if (index === -1) return [...order]
  const last = order.length - 1
  const target = action === 'up' ? index - 1
    : action === 'down' ? index + 1
    : action === 'top' ? 0
    : last
  if (target < 0 || target > last || target === index) return [...order]
  const next = [...order]
  next.splice(index, 1)
  next.splice(target, 0, name)
  return next
}

/**
 * 排序菜单 key 与移动动作同一词表（key 即 action，杜绝两套词表错位）：
 * up / down / top / bottom 四种移动 + reset 恢复默认排序。
 */
export function isSidebarSortAction(v: string): v is SidebarSortAction {
  return v === 'up' || v === 'down' || v === 'top' || v === 'bottom'
}

/**
 * 排序菜单选项构建纯函数（含组内边界置灰）：
 * 上移一位 / 下移一位 / 移到组内顶部 / 移到组内底部 /
 * 分隔线 / 恢复默认排序（恒可用）。
 * 置灰按传入组内序的边界判定：首位上移、移顶置灰；末位下移、移底置灰。
 */
export function buildSidebarSortMenuOptions(
  name: ViewName,
  order: readonly ViewName[],
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
    { label: t('common.sidebarSort.reset'), key: 'reset', disabled: false },
  ]
}

/** 点选即重排并立即持久化（写路径唯一出处；顺序变更经 viewShortcuts 自动随动）。
 *  只在 name 所属组内移动（组与组序固定，跨组移动不可达）；
 *  边界 no-op 不写存储：保住「恢复默认 = 删除 key」语义，出厂默认序将来调整时自动跟随。 */
export function applySidebarSort(name: ViewName, action: SidebarSortAction) {
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
// 键位按线性位置推导（issue #359 / ADR-0051；#372 键位收紧）：数字键位覆盖前 10 个
// 视图——数字键物理上限。概览恒 '1'；可排区 8 项按线性位置得 '2'..'9'（十键十视图
// 全占，「可排区末位无键位」wart 消除）；「更多」为第四固定项，不占键位（null）；
// AI 恒 '0'（第 10 位）；设置为唯一例外用 ','。组内重排键位随动、无死角（10 个数字
// 键恰好映射 10 个带键位视图）。
// ---------------------------------------------------------------------------

const LEAD_KEY = '1'
const ARRANGEABLE_KEYS = ['2', '3', '4', '5', '6', '7', '8', '9'] as const
const PENULTIMATE_KEY = '0'
const LAST_KEY = ','

/**
 * 键位推导纯函数：由组内序按线性位置派生全部视图快捷键（键随位置，重排即重排键位）。
 * 可排区恰 8 项时数字键位带 2..9 全占；「更多」不占键位（key: null）。
 */
export function deriveViewShortcuts(orders: SidebarGroupOrders): ViewShortcut[] {
  const shortcuts: ViewShortcut[] = [{ name: FIRST_VIEW, key: LEAD_KEY }]
  let i = 0
  for (const g of SIDEBAR_GROUPS) {
    for (const name of orders[g.id]) {
      shortcuts.push({ name, key: ARRANGEABLE_KEYS[i] ?? null })
      i++
    }
  }
  shortcuts.push({ name: EXTRA_VIEW, key: null })
  shortcuts.push({ name: PENULTIMATE_VIEW, key: PENULTIMATE_KEY })
  shortcuts.push({ name: LAST_VIEW, key: LAST_KEY })
  return shortcuts
}

/**
 * 视图快捷键映射（响应式）：由组内序按位置推导，键随位置（组内重排侧栏即重排键位）。
 * 每个视图恰一条记录（无键位视图 key 为 null）：数字 1–0 覆盖前 10 个视图，
 * 设置用 Cmd/Ctrl+,（macOS「设置」惯例键位，避免占用 Cmd+S 的「保存」肌肉记忆）。
 */
export const viewShortcuts = computed<ViewShortcut[]>(() => deriveViewShortcuts(groupOrders.value))

/** macOS 用 Cmd（metaKey），Windows/Linux 用 Ctrl（ctrlKey） */
export function isMacPlatform(): boolean {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } }
  const platform = nav.userAgentData?.platform ?? navigator.platform
  return /mac/i.test(platform)
}

/** 菜单提示文案：macOS 显示 ⌘1，其余显示 Ctrl+1 */
export function shortcutHint(key: string): string {
  return isMacPlatform() ? `⌘${key}` : `Ctrl+${key}`
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
