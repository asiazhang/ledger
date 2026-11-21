import {
  createRouter,
  createWebHashHistory,
  type LocationQuery,
  type NavigationGuard,
  type RouteRecordRaw,
  type Router,
} from "vue-router";
import { getSavedRouteName, saveRouteName } from "@ledger/utils/view-state";
import { useSidebarOrderStore } from "@/stores/sidebar-order";
import {
  isClosableFeature,
  useFeatureToggleStore,
  type ClosableFeatureId,
} from "@/settings/feature-toggles";
import { hasFocusParam } from "@/composables/useFocusParam";

function featureRouteGuard(
  view: ClosableFeatureId,
  containment?: { target: "assets-more" | "bookkeeping-more" | "insights-more"; tab: string },
): NavigationGuard {
  return (to) => {
    if (useFeatureToggleStore().isFeatureClosed(view)) {
      return hasFocusParam(to.query) ? true : { name: "dashboard" };
    }
    if (containment && useSidebarOrderStore().isViewContained(view)) {
      return { name: containment.target, query: { ...to.query, tab: containment.tab } };
    }
    return true;
  };
}

const groupMoreTabGuard: NavigationGuard = (to) => {
  const tab = to.query.tab;
  if (typeof tab !== "string" || !isClosableFeature(tab)) return true;
  if (!useFeatureToggleStore().isFeatureClosed(tab)) return true;
  const query: LocationQuery = { ...to.query };
  delete query.tab;
  if (tab === "scheduled" && typeof to.query.scheduledTab === "string") {
    query.tab = to.query.scheduledTab;
  }
  return { name: tab, query };
};

export const routes: RouteRecordRaw[] = [
  { path: "/", redirect: "/dashboard" },
  {
    path: "/dashboard",
    name: "dashboard",
    component: () => import("@/views/DashboardView.vue"),
  },
  {
    path: "/transactions",
    name: "transactions",
    component: () => import("@/views/TransactionsView.vue"),
  },
  {
    path: "/search",
    name: "search",
    component: () => import("@/views/SearchView.vue"),
  },
  {
    path: "/accounts",
    name: "accounts",
    component: () => import("@/views/AccountsView.vue"),
  },
  {
    path: "/reports",
    name: "reports",
    component: () => import("@/views/ReportsView.vue"),
    beforeEnter: featureRouteGuard("reports"),
  },
  {
    path: "/investments",
    name: "investments",
    component: () => import("@/views/InvestmentsView.vue"),
    beforeEnter: featureRouteGuard("investments"),
  },
  {
    path: "/cross-book-summary",
    name: "cross-book-summary",
    component: () => import("@/views/CrossBookSummaryView.vue"),
    beforeEnter: featureRouteGuard("investments"),
  },
  {
    path: "/scheduled",
    name: "scheduled",
    component: () => import("@/views/ScheduledView.vue"),
    beforeEnter: featureRouteGuard("scheduled"),
  },
  {
    path: "/savings-goals",
    name: "savingsGoals",
    component: () => import("@/views/SavingsGoalsView.vue"),
  },
  {
    path: "/subscriptions",
    name: "subscriptions",
    redirect: { name: "scheduled", query: { tab: "subscriptions" } },
  },
  {
    path: "/items",
    name: "items",
    component: () => import("@/views/ItemsView.vue"),
    beforeEnter: featureRouteGuard("items"),
  },
  {
    path: "/policies",
    name: "policies",
    component: () => import("@/views/PoliciesView.vue"),
    beforeEnter: featureRouteGuard("policies", { target: "assets-more", tab: "policies" }),
  },
  {
    path: "/merchants",
    name: "merchants",
    component: () => import("@/merchants/MerchantManager.vue"),
    beforeEnter: featureRouteGuard("merchants"),
  },
  {
    path: "/physical-assets",
    name: "physicalAssets",
    component: () => import("@/views/PhysicalAssetsView.vue"),
    beforeEnter: featureRouteGuard("physicalAssets"),
  },
  {
    path: "/insurers",
    name: "insurers",
    component: () => import("@/policy/InsurerManager.vue"),
    beforeEnter: featureRouteGuard("insurers", { target: "assets-more", tab: "insurers" }),
  },
  {
    path: "/more",
    name: "more",
    redirect: (to) =>
      to.query.tab === "policies"
        ? { name: "assets-more", query: to.query }
        : { name: "bookkeeping-more", query: to.query },
  },
  {
    path: "/bookkeeping/more",
    name: "bookkeeping-more",
    component: () => import("@/views/GroupMoreView.vue"),
    props: { group: "bookkeeping" },
    beforeEnter: groupMoreTabGuard,
  },
  {
    path: "/assets/more",
    name: "assets-more",
    component: () => import("@/views/GroupMoreView.vue"),
    props: { group: "assets" },
    beforeEnter: groupMoreTabGuard,
  },
  {
    path: "/insights/more",
    name: "insights-more",
    component: () => import("@/views/GroupMoreView.vue"),
    props: { group: "insights" },
    beforeEnter: groupMoreTabGuard,
  },
  {
    path: "/budget",
    name: "budget",
    component: () => import("@/views/BudgetView.vue"),
    beforeEnter: featureRouteGuard("budget"),
  },
  {
    path: "/ai",
    name: "ai",
    component: () => import("@/views/AiPromptView.vue"),
  },
  {
    path: "/settings",
    name: "settings",
    component: () => import("@/views/SettingsView.vue"),
  },
];

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

router.afterEach((to) => {
  if (typeof to.name === "string") saveRouteName(to.name);
});

export function resolveRestoredViewName(saved: string | null): string | null {
  if (saved !== null && useFeatureToggleStore().isFeatureClosed(saved)) return "dashboard";
  return saved;
}

export async function restoreLastView(router: Router): Promise<void> {
  const saved = resolveRestoredViewName(getSavedRouteName());
  if (saved && router.hasRoute(saved)) {
    await router.replace({ name: saved });
  }
}
