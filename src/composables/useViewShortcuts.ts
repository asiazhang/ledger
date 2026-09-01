import { computed, onMounted, onUnmounted, ref } from 'vue'
import type { Router } from 'vue-router'
import type { DropdownOption } from 'naive-ui'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import { getSavedSidebarOrder, saveSidebarOrders, clearSidebarOrder } from '@/utils/view-state'
import { t } from '@/i18n'

export interface ViewShortcut {
  /** 路由 name（与侧边栏菜单 key 一致） */
  name: string
  /** 主键：按线性位置推导的 '1'..'9'、'0'（AI）或 ','（设置）；null = 无键位（可排区末位） */
  key: string | null
}

/**
 * 顺序源模块：侧边栏视图顺序单一来源（顺序 = 菜单顺序 = 数字键位）。
 * 侧栏为分组形态（issue #359 / ADR-0051）：组与组序固定（记账/资产/洞察），组内序可排
 * （右键菜单）且持久化对象收窄为组内序；分组标题不占键位、不参与排序与计数，
 * 键位按线性位置推导——「位置即键位」哲学在数字键物理上限内的诚实延伸。
 */

/**
 * 三组（域职责分组，组 id 即 i18n key `common.sidebarGroup.<id>`）。
 * 组与组序固定、成员闭集；「资产」组的保单空位就绪（随保单建档票接入为组内末位）——
 * 接入后可排区扩为九项，键位带末位（第 9 位）无键位，右键重排即换谁无键位。
 */
export const SIDEBAR_GROUPS = [
  { id: 'bookkeeping', views: ['transactions', 'accounts', 'budget', 'scheduled'] },
  { id: 'assets', views: ['investments', 'items'] },
  { id: 'insights', views: ['reports', 'search'] },
] as const

export type SidebarGroupId = (typeof SIDEBAR_GROUPS)[number]['id']

/** 三固定项：概览首位（与启动落地页一致）、AI 倒数第二、设置末位 */
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

/** 可排区（组内）：各组成员按组序展开，相对顺序即默认相对顺序 */
export const ARRANGEABLE_VIEWS: readonly ViewName[] = SIDEBAR_GROUPS.flatMap((g) => [...g.views])

/** 固定项例外判定：可排区八项为真，概览/AI/设置三固定项为假（右键无菜单）。 */
export function isArrangeableView(v: unknown): v is ViewName {
  return typeof v === 'string' && (ARRANGEABLE_VIEWS as readonly string[]).includes(v)
}

/** 视图 → 所属组（可排区八项各有其组；概览/AI/设置与未知名不在任何组，返回 null）。 */
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

/** 恢复默认排序：清除存储回出厂序（之后可再次自定义，反复交替）。 */
export function resetSidebarOrder() {
  clearSidebarOrder()
  groupOrders.value = defaultGroupOrders()
}

// ---------------------------------------------------------------------------
// 键位按线性位置推导（issue #359 / ADR-0051）：数字键位覆盖前 10 个视图——数字键
// 物理上限的诚实处理。概览恒 '1'；可排区第 1–8 项按线性位置得 '2'..'9'；AI 恒 '0'
// （第 10 位）；设置为唯一例外用 ','。可排区容量九项（「资产」组保单空位就绪）：
// 第 9 项落在键位带之外——末位无键位，不出提示、键盘不可跳转，右键重排即换谁无键位。
// ---------------------------------------------------------------------------

const LEAD_KEY = '1'
const ARRANGEABLE_KEYS = ['2', '3', '4', '5', '6', '7', '8', '9'] as const
const PENULTIMATE_KEY = '0'
const LAST_KEY = ','

/**
 * 键位推导纯函数：由组内序按线性位置派生全部视图快捷键（键随位置，重排即重排键位）。
 * 可排区超出 8 项时（第 9 项起）无键位（key: null）。
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
