import { computed, onMounted, onUnmounted } from "vue";
import type { Router } from "vue-router";
import { hasOpenOverlay } from "@ledger/ui-kit/overlayRegistry";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import {
  useSidebarOrderStore,
  FIRST_VIEW,
  PENULTIMATE_VIEW,
  LAST_VIEW,
  SIDEBAR_GROUPS,
} from "@/stores/sidebar-order";
import type { SidebarGroupId, SidebarGroupOrders } from "@/stores/sidebar-order";

export interface ViewShortcut {
  name: string;
  key: string | null;
}

const LEAD_KEY = "`";
const GROUP_KEY_BANDS: Record<SidebarGroupId, readonly string[]> = {
  bookkeeping: ["1", "2", "3"],
  assets: ["4", "5", "6"],
  insights: ["7", "8", "9"],
};
const PENULTIMATE_KEY = "0";
const LAST_KEY = ",";

export function deriveViewShortcuts(
  orders: SidebarGroupOrders,
  closedFeatures: readonly string[] = [],
): ViewShortcut[] {
  const closed = new Set(closedFeatures);
  const shortcuts: ViewShortcut[] = [{ name: FIRST_VIEW, key: LEAD_KEY }];
  for (const g of SIDEBAR_GROUPS) {
    const band = GROUP_KEY_BANDS[g.id];
    orders[g.id].forEach((name, i) => {
      if (closed.has(name)) return;
      shortcuts.push({ name, key: band[i] ?? null });
    });
  }
  shortcuts.push({ name: PENULTIMATE_VIEW, key: PENULTIMATE_KEY });
  shortcuts.push({ name: LAST_VIEW, key: LAST_KEY });
  return shortcuts;
}

function isMacPlatform(): boolean {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = nav.userAgentData?.platform ?? navigator.platform;
  return /mac/i.test(platform);
}

export function shortcutHint(key: string): string {
  return isMacPlatform() ? `⌘${key}` : `⌃${key}`;
}

function isPrimaryModifier(e: KeyboardEvent): boolean {
  return isMacPlatform() ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
}

export function matchViewShortcut(e: KeyboardEvent): string | null {
  if (e.altKey || e.shiftKey) return null;
  if (!isPrimaryModifier(e)) return null;
  const shortcuts = deriveViewShortcuts(
    useSidebarOrderStore().sidebarGroupOrders,
    useFeatureToggleStore().closedFeatures,
  );
  return shortcuts.find((s) => s.key !== null && e.key === s.key)?.name ?? null;
}

export function useViewShortcuts(router: Router) {
  const store = useSidebarOrderStore();
  const featureToggles = useFeatureToggleStore();
  const viewShortcuts = computed<ViewShortcut[]>(() =>
    deriveViewShortcuts(store.sidebarGroupOrders, featureToggles.closedFeatures),
  );

  const onKeydown = (e: KeyboardEvent) => {
    const name = matchViewShortcut(e);
    if (!name) return;
    if (hasOpenOverlay()) return;
    e.preventDefault();
    if (router.currentRoute.value.name !== name) {
      router.push({ name });
    }
  };
  onMounted(() => window.addEventListener("keydown", onKeydown));
  onUnmounted(() => window.removeEventListener("keydown", onKeydown));
  return { viewShortcuts };
}
