<script setup lang="ts">
import { computed, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { t } from "@ledger/i18n";
import { NTabs, NTabPane, NIcon } from "naive-ui";
import { CalendarOutline, PulseOutline, SyncOutline } from "@vicons/ionicons5";
import SubscriptionsPane from "@/scheduled/SubscriptionsPane.vue";
import InstallmentsPane from "@/scheduled/InstallmentsPane.vue";
import TransfersPane from "@/scheduled/TransfersPane.vue";
import { useFocusParam } from "@/composables/useFocusParam";
import type { ScheduledFormTab } from "@/components/source-jump";

const TABS: readonly ScheduledFormTab[] = ["subscriptions", "installments", "transfers"];
type ScheduledTab = ScheduledFormTab;

const props = defineProps<{ embedded?: boolean }>();

const route = useRoute();
const router = useRouter();

function isScheduledTab(v: unknown): v is ScheduledTab {
  return typeof v === "string" && (TABS as readonly string[]).includes(v);
}

const localTab = ref<ScheduledTab>("subscriptions");

if (props.embedded) {
  const landingTab = route.query.scheduledTab;
  if (isScheduledTab(landingTab)) localTab.value = landingTab;
}

const pendingFocusPlanId = ref<string | null>(null);
const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (planId) => {
    pendingFocusPlanId.value = planId;
  },
});
focusParam.consume();

function onPlanFocusConsumed() {
  pendingFocusPlanId.value = null;
}

const activeTab = computed<ScheduledTab>(() => {
  if (props.embedded) return localTab.value;
  return isScheduledTab(route.query.tab) ? route.query.tab : "subscriptions";
});

function onTabChange(key: string | number) {
  const tab = String(key);
  if (!isScheduledTab(tab) || tab === activeTab.value) return;
  if (props.embedded) {
    localTab.value = tab;
    return;
  }
  void router.replace({ query: { ...route.query, tab } });
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane name="subscriptions">
      <template #tab
        ><span class="pane-tab"
          ><NIcon :component="CalendarOutline" />{{ t("scheduled.tab.subscriptions") }}</span
        ></template
      >
      <SubscriptionsPane
        :focus-plan-id="pendingFocusPlanId"
        @focus-consumed="onPlanFocusConsumed"
      />
    </NTabPane>
    <NTabPane name="installments">
      <template #tab
        ><span class="pane-tab"
          ><NIcon :component="PulseOutline" />{{ t("scheduled.tab.installments") }}</span
        ></template
      >
      <InstallmentsPane :focus-plan-id="pendingFocusPlanId" @focus-consumed="onPlanFocusConsumed" />
    </NTabPane>
    <NTabPane name="transfers">
      <template #tab
        ><span class="pane-tab"
          ><NIcon :component="SyncOutline" />{{ t("scheduled.tab.transfers") }}</span
        ></template
      >
      <TransfersPane :focus-plan-id="pendingFocusPlanId" @focus-consumed="onPlanFocusConsumed" />
    </NTabPane>
  </NTabs>
</template>

<style scoped>
.pane-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
