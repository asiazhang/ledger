// 视图状态（ViewState）持久化：当前视图、侧边栏折叠、报表汇总层级。
// 约定：key 统一加 'view_state:' 前缀，与偏好（'appearance' 等）及业务数据（SQLite）分域。
// 边界：只覆盖上述三样，不做"过度记忆"（筛选、滚动位置、列宽等一律不持久化）。

import { loadLocal, saveLocal } from '@/utils/storage'

export const VIEW_STATE_KEYS = {
  route: 'view_state:route',
  sidebarCollapsed: 'view_state:sidebar_collapsed',
  reportsGroupLevel: 'view_state:reports_group_level',
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

/** 报表汇总层级；非法值一律回退 'level2'（二级）。 */
export function loadReportsGroupLevel(): ReportsGroupLevel {
  const v = loadLocal<ReportsGroupLevel>(VIEW_STATE_KEYS.reportsGroupLevel, 'level2')
  return v === 'level1' ? 'level1' : 'level2'
}

export function saveReportsGroupLevel(level: ReportsGroupLevel) {
  saveLocal(VIEW_STATE_KEYS.reportsGroupLevel, level)
}
