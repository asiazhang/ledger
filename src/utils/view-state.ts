// 视图状态（ViewState）持久化：当前视图、侧边栏折叠、报表汇总层级、侧栏可排区顺序。
// 约定：key 统一加 'view_state:' 前缀，与偏好（'appearance' 等）及业务数据（SQLite）分域。
// 边界：不做"过度记忆"（筛选、滚动位置、列宽等一律不持久化）。

import { loadLocal, saveLocal, removeLocal } from '@/utils/storage'
import type { ViewName } from '@/composables/useViewShortcuts'

export const VIEW_STATE_KEYS = {
  route: 'view_state:route',
  sidebarCollapsed: 'view_state:sidebar_collapsed',
  reportsGroupLevel: 'view_state:reports_group_level',
  sidebarOrder: 'view_state:sidebar_order',
} as const

export type ReportsGroupLevel = 'level1' | 'level2'

/** 上次所在视图的路由 name；无记录或数据损坏时返回 null（由调用方回退默认路由）。 */
export function getSavedRouteName(): string | null {
  const v = loadLocal<unknown>(VIEW_STATE_KEYS.route, null)
  return typeof v === 'string' ? v : null
}

export function saveRouteName(name: string) {
  saveLocal(VIEW_STATE_KEYS.route, name)
}

export function loadSidebarCollapsed(): boolean {
  return loadLocal<boolean>(VIEW_STATE_KEYS.sidebarCollapsed, false)
}

export function saveSidebarCollapsed(collapsed: boolean) {
  saveLocal(VIEW_STATE_KEYS.sidebarCollapsed, collapsed)
}

/**
 * 已存侧栏可排区顺序（原始值）；无记录或数据损坏时返回 null。
 * 非法名过滤、去重、缺失项补齐等解析防御归顺序模块 parseArrangeableOrder，此处不解析。
 */
export function getSavedSidebarOrder(): unknown {
  return loadLocal<unknown>(VIEW_STATE_KEYS.sidebarOrder, null)
}

/** 持久化自定义可排区顺序（点选即写，写路径唯一出处，issue #270）。 */
export function saveSidebarOrder(order: readonly ViewName[]) {
  saveLocal(VIEW_STATE_KEYS.sidebarOrder, order)
}

/** 清除自定义顺序（恢复默认排序），回退无记录态。 */
export function clearSidebarOrder() {
  removeLocal(VIEW_STATE_KEYS.sidebarOrder)
}

/** 报表汇总层级；非法值一律回退 'level2'（二级）。 */
export function loadReportsGroupLevel(): ReportsGroupLevel {
  const v = loadLocal<ReportsGroupLevel>(VIEW_STATE_KEYS.reportsGroupLevel, 'level2')
  return v === 'level1' ? 'level1' : 'level2'
}

export function saveReportsGroupLevel(level: ReportsGroupLevel) {
  saveLocal(VIEW_STATE_KEYS.reportsGroupLevel, level)
}
