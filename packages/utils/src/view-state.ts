import { loadLocal, saveLocal, removeLocal } from "@ledger/storage";

export const VIEW_STATE_KEYS = {
  route: "view_state:route",
  sidebarCollapsed: "view_state:sidebar_collapsed",
  sidebarOrder: "view_state:sidebar_order",
  sidebarContainment: "view_state:sidebar_containment",
  closedFeatures: "view_state:closed_features",
  hideInvestmentRelated: "view_state:hide_investment_related",
} as const;

export function getSavedRouteName(): string | null {
  const v = loadLocal<unknown>(VIEW_STATE_KEYS.route, null);
  return typeof v === "string" ? v : null;
}

export function saveRouteName(name: string) {
  saveLocal(VIEW_STATE_KEYS.route, name);
}

export function loadSidebarCollapsed(): boolean {
  return loadLocal<boolean>(VIEW_STATE_KEYS.sidebarCollapsed, false);
}

export function saveSidebarCollapsed(collapsed: boolean) {
  saveLocal(VIEW_STATE_KEYS.sidebarCollapsed, collapsed);
}

export function getSavedSidebarOrder(): unknown {
  return loadLocal<unknown>(VIEW_STATE_KEYS.sidebarOrder, null);
}

export function saveSidebarOrders(orders: unknown) {
  saveLocal(VIEW_STATE_KEYS.sidebarOrder, orders);
}

export function clearSidebarOrder() {
  removeLocal(VIEW_STATE_KEYS.sidebarOrder);
}

export function getSavedContainment(): unknown {
  return loadLocal<unknown>(VIEW_STATE_KEYS.sidebarContainment, null);
}

export function saveContainmentLists(lists: unknown) {
  saveLocal(VIEW_STATE_KEYS.sidebarContainment, lists);
}

export function clearContainment() {
  removeLocal(VIEW_STATE_KEYS.sidebarContainment);
}

export function getSavedClosedFeatures(): unknown {
  return loadLocal<unknown>(VIEW_STATE_KEYS.closedFeatures, null);
}

export function saveClosedFeatures(list: unknown) {
  saveLocal(VIEW_STATE_KEYS.closedFeatures, list);
}

export function clearClosedFeatures() {
  removeLocal(VIEW_STATE_KEYS.closedFeatures);
}

export function getSavedHideInvestmentRelated(): unknown {
  return loadLocal<unknown>(VIEW_STATE_KEYS.hideInvestmentRelated, null);
}

export function saveHideInvestmentRelated(value: unknown) {
  saveLocal(VIEW_STATE_KEYS.hideInvestmentRelated, value);
}

export function clearHideInvestmentRelated() {
  removeLocal(VIEW_STATE_KEYS.hideInvestmentRelated);
}
