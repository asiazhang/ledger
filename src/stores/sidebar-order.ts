import { computed, h, ref } from "vue";
import type { DropdownOption } from "naive-ui";
import { defineStore } from "pinia";
import {
  getSavedSidebarOrder,
  saveSidebarOrders,
  clearSidebarOrder,
  getSavedContainment,
  saveContainmentLists,
  clearContainment,
} from "@ledger/utils/view-state";
import { t } from "@ledger/i18n";

export const SIDEBAR_GROUPS = [
  { id: "bookkeeping", views: ["transactions", "accounts", "budget"] },
  { id: "assets", views: ["investments", "items", "savingsGoals"] },
  { id: "insights", views: ["reports", "search"] },
] as const;

export const GROUP_MAIN_LIMIT = 3;

export type SidebarGroupId = (typeof SIDEBAR_GROUPS)[number]["id"];

export const FIRST_VIEW = "dashboard";
export const PENULTIMATE_VIEW = "ai";
export const LAST_VIEW = "settings";

export const DEFAULT_VIEW_ORDER = [
  FIRST_VIEW,
  ...SIDEBAR_GROUPS.flatMap((g) => g.views),
  PENULTIMATE_VIEW,
  LAST_VIEW,
] as const;

export type ViewName = (typeof DEFAULT_VIEW_ORDER)[number];

export const ARRANGEABLE_VIEWS = SIDEBAR_GROUPS.flatMap((g) => [...g.views]);

export function isArrangeableView(v: unknown): v is ViewName {
  return typeof v === "string" && (ARRANGEABLE_VIEWS as readonly string[]).includes(v);
}

export function groupOfView(name: AnyViewName): SidebarGroupId | null {
  for (const g of SIDEBAR_GROUPS) {
    if ((g.views as readonly string[]).includes(name)) return g.id;
  }
  for (const g of SIDEBAR_GROUPS) {
    if ((GROUP_CONTAINMENT_SEEDS[g.id] as readonly string[]).includes(name)) return g.id;
  }
  return null;
}

export type SidebarGroupOrders = Readonly<Record<SidebarGroupId, readonly ContainableViewName[]>>;

function defaultGroupOrders(): Record<SidebarGroupId, ContainableViewName[]> {
  const result = {} as Record<SidebarGroupId, ContainableViewName[]>;
  for (const g of SIDEBAR_GROUPS) result[g.id] = [...g.views];
  return result;
}

export function parseGroupOrders(
  raw: unknown,
  contained: Readonly<Record<SidebarGroupId, readonly string[]>> = defaultContainmentLists(),
): Record<SidebarGroupId, ContainableViewName[]> {
  const result = defaultGroupOrders();
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return result;
  const rec = raw as Record<string, unknown>;
  for (const g of SIDEBAR_GROUPS) {
    const excluded = contained[g.id] ?? [];
    const members = [
      ...(g.views as readonly string[]),
      ...(GROUP_CONTAINMENT_SEEDS[g.id] as readonly string[]),
    ].filter((v) => !excluded.includes(v));
    const kept: ContainableViewName[] = [];
    const seen = new Set<string>();
    if (Array.isArray(rec[g.id])) {
      for (const item of rec[g.id] as unknown[]) {
        if (typeof item !== "string" || !members.includes(item) || seen.has(item)) continue;
        seen.add(item);
        kept.push(item as ContainableViewName);
      }
    }
    for (const name of members) {
      if (!seen.has(name)) kept.push(name as ContainableViewName);
    }
    result[g.id] = kept;
  }
  return result;
}

export const GROUP_CONTAINMENT_SEEDS = {
  bookkeeping: ["scheduled", "merchants"],
  assets: ["policies", "physicalAssets", "insurers"],
  insights: [],
} as const satisfies Record<SidebarGroupId, readonly string[]>;

export type ContainableViewName =
  | (typeof GROUP_CONTAINMENT_SEEDS)[SidebarGroupId][number]
  | (typeof ARRANGEABLE_VIEWS)[number];

export type SidebarContainmentLists = Readonly<
  Record<SidebarGroupId, readonly ContainableViewName[]>
>;

export type AnyViewName = ViewName | ContainableViewName;

function defaultContainmentLists(): Record<SidebarGroupId, ContainableViewName[]> {
  const result = {} as Record<SidebarGroupId, ContainableViewName[]>;
  for (const g of SIDEBAR_GROUPS) result[g.id] = [...GROUP_CONTAINMENT_SEEDS[g.id]];
  return result;
}

export function parseContainmentLists(
  raw: unknown,
  rawOrders: unknown = null,
): Record<SidebarGroupId, ContainableViewName[]> {
  const result = defaultContainmentLists();
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return result;
  const rec = raw as Record<string, unknown>;
  const orderRec =
    typeof rawOrders === "object" && rawOrders !== null && !Array.isArray(rawOrders)
      ? (rawOrders as Record<string, unknown>)
      : null;
  for (const g of SIDEBAR_GROUPS) {
    const legal = [...GROUP_CONTAINMENT_SEEDS[g.id], ...(g.views as readonly string[])];
    const kept: ContainableViewName[] = [];
    const seen = new Set<string>();
    const rawArr = Array.isArray(rec[g.id]) ? (rec[g.id] as unknown[]) : null;
    if (rawArr) {
      for (const item of rawArr) {
        if (typeof item !== "string" || !legal.includes(item) || seen.has(item)) continue;
        seen.add(item);
        kept.push(item as ContainableViewName);
      }
    }
    const orderArr =
      orderRec && Array.isArray(orderRec[g.id]) ? (orderRec[g.id] as unknown[]) : null;
    for (const name of GROUP_CONTAINMENT_SEEDS[g.id]) {
      if (seen.has(name)) continue;
      if (rawArr && orderArr?.includes(name)) continue;
      kept.push(name as ContainableViewName);
    }
    result[g.id] = kept;
  }
  return result;
}

export function moveIntoContainment(
  lists: SidebarContainmentLists,
  gid: SidebarGroupId,
  name: ContainableViewName,
): SidebarContainmentLists {
  const list = lists[gid];
  if (list.includes(name)) return lists;
  return { ...lists, [gid]: [...list, name] };
}

export function moveBackToSidebar(
  lists: SidebarContainmentLists,
  gid: SidebarGroupId,
  name: ContainableViewName,
): SidebarContainmentLists {
  const list = lists[gid];
  if (!list.includes(name)) return lists;
  return { ...lists, [gid]: list.filter((v) => v !== name) };
}

export function isGroupFull(order: readonly ContainableViewName[]): boolean {
  return order.length >= GROUP_MAIN_LIMIT;
}

export type SidebarSortAction = "up" | "down" | "top" | "bottom";

export function moveArrangeable(
  order: readonly ContainableViewName[],
  name: AnyViewName,
  action: SidebarSortAction,
): ContainableViewName[] {
  const index = (order as readonly string[]).indexOf(name);
  if (index === -1) return [...order];
  const last = order.length - 1;
  const target =
    action === "up" ? index - 1 : action === "down" ? index + 1 : action === "top" ? 0 : last;
  if (target < 0 || target > last || target === index) return [...order];
  const next = [...order];
  next.splice(index, 1);
  next.splice(target, 0, name as ContainableViewName);
  return next;
}

export function isSidebarSortAction(v: string): v is SidebarSortAction {
  return v === "up" || v === "down" || v === "top" || v === "bottom";
}

export function buildSidebarSortMenuOptions(
  name: ContainableViewName,
  order: readonly ContainableViewName[],
): DropdownOption[] {
  const index = order.indexOf(name);
  const atTop = index <= 0;
  const atBottom = index === order.length - 1;
  return [
    { label: t("common.sidebarSort.up"), key: "up", disabled: atTop },
    { label: t("common.sidebarSort.down"), key: "down", disabled: atBottom },
    { label: t("common.sidebarSort.top"), key: "top", disabled: atTop },
    { label: t("common.sidebarSort.bottom"), key: "bottom", disabled: atBottom },
    { type: "divider", key: "sort-divider" },
    { label: t("common.sidebarContainment.intoMore"), key: "intoMore", disabled: false },
    { type: "divider", key: "reset-divider" },
    { label: t("common.sidebarSort.reset"), key: "reset", disabled: false },
  ];
}

export function buildTabContextMenuOptions(
  order: readonly ContainableViewName[],
): DropdownOption[] {
  const full = isGroupFull(order);
  const label: DropdownOption["label"] = full
    ? () =>
        h("div", null, [
          h("div", null, t("common.sidebarContainment.backToSidebar")),
          h(
            "div",
            {
              style: "font-size:12px;line-height:1.5;opacity:.6;white-space:normal;max-width:220px",
            },
            t("common.sidebarContainment.backBlockedFull"),
          ),
        ])
    : t("common.sidebarContainment.backToSidebar");
  return [{ key: "backToSidebar", disabled: full, label }];
}

export const useSidebarOrderStore = defineStore("sidebar-order", () => {
  const savedSidebarOrderRaw = getSavedSidebarOrder();
  const containmentLists = ref<SidebarContainmentLists>(
    parseContainmentLists(getSavedContainment(), savedSidebarOrderRaw),
  );

  const groupOrders = ref<Record<SidebarGroupId, ContainableViewName[]>>(
    parseGroupOrders(savedSidebarOrderRaw, containmentLists.value),
  );

  const sidebarGroupOrders = computed<SidebarGroupOrders>(() => groupOrders.value);

  const sidebarContainment = computed<SidebarContainmentLists>(() => containmentLists.value);

  const sidebarGroups = computed<
    readonly { id: SidebarGroupId; views: readonly ContainableViewName[] }[]
  >(() => SIDEBAR_GROUPS.map((g) => ({ id: g.id, views: groupOrders.value[g.id] })));

  function isSidebarMember(v: unknown): v is ContainableViewName {
    if (typeof v !== "string") return false;
    return SIDEBAR_GROUPS.some((g) => (groupOrders.value[g.id] as readonly string[]).includes(v));
  }

  function isViewContained(name: ContainableViewName): boolean {
    return SIDEBAR_GROUPS.some((g) =>
      (containmentLists.value[g.id] as readonly string[]).includes(name),
    );
  }

  function applyMoveIntoMore(name: AnyViewName) {
    const gid = groupOfView(name);
    if (!gid) return;
    const prev = groupOrders.value[gid];
    if (!(prev as readonly string[]).includes(name)) return;
    groupOrders.value = { ...groupOrders.value, [gid]: prev.filter((v) => v !== name) };
    containmentLists.value = moveIntoContainment(
      containmentLists.value,
      gid,
      name as ContainableViewName,
    );
    saveSidebarOrders(groupOrders.value);
    saveContainmentLists(containmentLists.value);
  }

  function applyMoveBackToSidebar(name: AnyViewName) {
    const gid = groupOfView(name);
    if (!gid) return;
    const list = containmentLists.value[gid];
    if (!(list as readonly string[]).includes(name)) return;
    const order = groupOrders.value[gid];
    if (isGroupFull(order)) return;
    const member = name as ContainableViewName;
    containmentLists.value = moveBackToSidebar(containmentLists.value, gid, member);
    groupOrders.value = { ...groupOrders.value, [gid]: [...order, member] };
    saveContainmentLists(containmentLists.value);
    saveSidebarOrders(groupOrders.value);
  }

  function applySidebarSort(name: AnyViewName, action: SidebarSortAction) {
    const gid = groupOfView(name);
    if (!gid) return;
    const prev = groupOrders.value[gid];
    const next = moveArrangeable(prev, name, action);
    if (prev.every((v, i) => v === next[i])) return;
    groupOrders.value = { ...groupOrders.value, [gid]: next };
    saveSidebarOrders(groupOrders.value);
  }

  function resetSidebarOrder() {
    clearSidebarOrder();
    clearContainment();
    groupOrders.value = defaultGroupOrders();
    containmentLists.value = defaultContainmentLists();
  }

  return {
    groupOrders,
    containmentLists,
    sidebarGroupOrders,
    sidebarContainment,
    sidebarGroups,
    isSidebarMember,
    isViewContained,
    applyMoveIntoMore,
    applyMoveBackToSidebar,
    applySidebarSort,
    resetSidebarOrder,
  };
});
