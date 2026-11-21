<script setup lang="ts">
import { computed, h, nextTick, ref, type Component, type VNode } from "vue";
import { useRoute, useRouter } from "vue-router";
import { t } from "@ledger/i18n";
import { NTabs, NTabPane, NIcon } from "naive-ui";
import AppDropdown from "@ledger/ui-kit/AppDropdown.vue";
import {
  ShieldCheckmarkOutline,
  CubeOutline,
  RepeatOutline,
  StorefrontOutline,
  SwapHorizontalOutline,
  WalletOutline,
  CalculatorOutline,
  TrendingUpOutline,
  BarChartOutline,
  SearchOutline,
  UmbrellaOutline,
  FlagOutline,
} from "@vicons/ionicons5";
import PoliciesView from "@/views/PoliciesView.vue";
import PhysicalAssetsView from "@/views/PhysicalAssetsView.vue";
import ScheduledView from "@/views/ScheduledView.vue";
import MerchantManager from "@/merchants/MerchantManager.vue";
import InsurerManager from "@/policy/InsurerManager.vue";
import TransactionsView from "@/views/TransactionsView.vue";
import AccountsView from "@/views/AccountsView.vue";
import BudgetView from "@/views/BudgetView.vue";
import InvestmentsView from "@/views/InvestmentsView.vue";
import ItemsView from "@/views/ItemsView.vue";
import ReportsView from "@/views/ReportsView.vue";
import SearchView from "@/views/SearchView.vue";
import SavingsGoalsView from "@/views/SavingsGoalsView.vue";
import { useSidebarOrderStore, buildTabContextMenuOptions } from "@/stores/sidebar-order";
import { useFeatureToggleStore } from "@/settings/feature-toggles";
import { useWindowTier } from "@ledger/window-tier";
import type { ContainableViewName, SidebarGroupId } from "@/stores/sidebar-order";

const props = defineProps<{ group: SidebarGroupId }>();

const CONTAINED_VIEWS: Record<ContainableViewName, { component: Component; icon: Component }> = {
  policies: { component: PoliciesView, icon: ShieldCheckmarkOutline },
  physicalAssets: { component: PhysicalAssetsView, icon: CubeOutline },
  insurers: { component: InsurerManager, icon: UmbrellaOutline },
  scheduled: { component: () => h(ScheduledView, { embedded: true }), icon: RepeatOutline },
  merchants: { component: MerchantManager, icon: StorefrontOutline },
  transactions: { component: TransactionsView, icon: SwapHorizontalOutline },
  accounts: { component: AccountsView, icon: WalletOutline },
  budget: { component: BudgetView, icon: CalculatorOutline },
  investments: { component: InvestmentsView, icon: TrendingUpOutline },
  items: { component: ItemsView, icon: CubeOutline },
  savingsGoals: { component: SavingsGoalsView, icon: FlagOutline },
  reports: { component: ReportsView, icon: BarChartOutline },
  search: { component: SearchView, icon: SearchOutline },
};

const route = useRoute();
const router = useRouter();

const windowTier = useWindowTier();
const isMobileTier = computed(() => windowTier.value === "mobile");

const sidebarOrder = useSidebarOrderStore();
const { applyMoveBackToSidebar } = sidebarOrder;
const featureToggles = useFeatureToggleStore();

const tabs = computed(() =>
  sidebarOrder.sidebarContainment[props.group].filter(
    (name) => !featureToggles.isFeatureClosed(name),
  ),
);

function isLegalTab(v: unknown): v is string {
  return typeof v === "string" && (tabs.value as readonly string[]).includes(v);
}

const activeTab = computed(() => (isLegalTab(route.query.tab) ? route.query.tab : tabs.value[0]));

function onTabChange(key: string | number) {
  const tab = String(key);
  if (isLegalTab(tab) && tab !== activeTab.value) {
    void router.replace({ query: { ...route.query, tab } });
  }
}

const backMenuShow = ref(false);
const backMenuX = ref(0);
const backMenuY = ref(0);
const backTarget = ref<ContainableViewName | null>(null);

const backMenuOptions = computed(() =>
  buildTabContextMenuOptions(sidebarOrder.sidebarGroupOrders[props.group]),
);

function onTabContextmenu(e: MouseEvent, name: ContainableViewName) {
  if (isMobileTier.value) return;
  backTarget.value = name;
  backMenuX.value = e.clientX;
  backMenuY.value = e.clientY;
  backMenuShow.value = false;
  void nextTick(() => {
    backMenuShow.value = true;
  });
}

function onBackMenuSelect(key: string) {
  backMenuShow.value = false;
  const target = backTarget.value;
  if (key !== "backToSidebar" || !target) return;
  applyMoveBackToSidebar(target);
}

function renderBackOptionRow({ node }: { node: VNode }): VNode {
  return h("div", { class: "tab-back-option" }, [node]);
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane v-for="name in tabs" :key="name" :name="name">
      <template #tab
        ><span class="pane-tab" @contextmenu="onTabContextmenu($event, name)"
          ><NIcon :component="CONTAINED_VIEWS[name].icon" />{{ t(`common.nav.${name}`) }}</span
        ></template
      >
      <component :is="CONTAINED_VIEWS[name].component" />
    </NTabPane>
  </NTabs>
  <!-- 页签右键「移回侧栏」菜单（issue #475）：手动定位弹出，与侧栏排序菜单同一封装 -->
  <AppDropdown
    trigger="manual"
    placement="bottom-start"
    :show="backMenuShow"
    :x="backMenuX"
    :y="backMenuY"
    :options="backMenuOptions"
    style="max-width: 140px"
    :render-option="renderBackOptionRow"
    @select="onBackMenuSelect"
    @clickoutside="backMenuShow = false"
  />
</template>

<style scoped>
.pane-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
