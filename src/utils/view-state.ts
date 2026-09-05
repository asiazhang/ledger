// 视图状态（ViewState）持久化：当前视图、侧边栏折叠、侧栏组内顺序（issue #359）、
// 每组收纳清单（issue #472 / ADR-0063）。
// 约定：key 统一加 'view_state:' 前缀，与偏好（'appearance' 等）及业务数据（SQLite）分域。
// 边界：不做"过度记忆"（筛选、滚动位置、列宽等一律不持久化）。

import { loadLocal, saveLocal, removeLocal } from '@/utils/storage'

export const VIEW_STATE_KEYS = {
  route: 'view_state:route',
  sidebarCollapsed: 'view_state:sidebar_collapsed',
  sidebarOrder: 'view_state:sidebar_order',
  sidebarContainment: 'view_state:sidebar_containment',
} as const

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
 * 已存侧栏组内序（原始值）；无记录或数据损坏时返回 null。
 * 旧平铺数组（issue #270 形态）等脏形状的整体回退、组内非法名过滤、去重、缺失项补齐等
 * 解析防御归 sidebar-order store parseGroupOrders，此处不解析。
 */
export function getSavedSidebarOrder(): unknown {
  return loadLocal<unknown>(VIEW_STATE_KEYS.sidebarOrder, null)
}

/** 持久化组内序（点选即写，写路径唯一出处，issue #270/#359）：对象形状「组 id → 视图名数组」。
 *  参数透传 unknown：类型耦合经透传消解（issue #549），词表与形状守卫归 sidebar-order store。 */
export function saveSidebarOrders(orders: unknown) {
  saveLocal(VIEW_STATE_KEYS.sidebarOrder, orders)
}

/** 清除自定义顺序（恢复默认排序），回退无记录态。 */
export function clearSidebarOrder() {
  removeLocal(VIEW_STATE_KEYS.sidebarOrder)
}

/**
 * 已存每组收纳清单（原始值，issue #472 / ADR-0063）；无记录或数据损坏时返回 null。
 * 与组内序同族：脏形状整体回出厂种子、非法名过滤、去重、缺失成员补尾等解析防御
 * 归 sidebar-order store parseContainmentLists，此处不解析。
 */
export function getSavedContainment(): unknown {
  return loadLocal<unknown>(VIEW_STATE_KEYS.sidebarContainment, null)
}

/** 持久化每组收纳清单（写路径唯一出处）：对象形状「组 id → 收纳视图名数组」，清单序 = 页签序。
 *  参数透传 unknown：类型耦合经透传消解（issue #549），词表与形状守卫归 sidebar-order store。 */
export function saveContainmentLists(lists: unknown) {
  saveLocal(VIEW_STATE_KEYS.sidebarContainment, lists)
}

/** 清除收纳清单存储（恢复默认排序连收纳一起复位），回退无记录态。 */
export function clearContainment() {
  removeLocal(VIEW_STATE_KEYS.sidebarContainment)
}
