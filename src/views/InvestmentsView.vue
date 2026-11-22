<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, type LocationQuery } from "vue-router";
import { NAlert, NButton, NIcon, NTabPane, NTabs, NText } from "naive-ui";
import {
  SpeedometerOutline,
  StatsChartOutline,
  ListOutline,
  DocumentTextOutline,
  PieChartOutline,
} from "@vicons/ionicons5";
import { t } from "@ledger/i18n";
import { useFocusParam } from "@/composables/useFocusParam";
import { usePriceStaleness } from "@/investment/usePriceStaleness";
import { registerViewReset } from "@/composables/viewResetRegistry";
import { useInvestmentsSessionStore } from "@/investment/investments-session";
import InvestmentOverviewPanel from "@/investment/InvestmentOverviewPanel.vue";
import RealizedPnlPanel from "@/investment/RealizedPnlPanel.vue";
import HoldingsOverview from "@/investment/HoldingsOverview.vue";
import InvestmentLedgerTab from "@/investment/InvestmentLedgerTab.vue";
import HistoryBackfillIndicator from "@/investment/HistoryBackfillIndicator.vue";
import InstrumentBrowser from "@/investment/InstrumentBrowser.vue";

const route = useRoute();

const session = useInvestmentsSessionStore();
const activeTab = computed(() => session.activeTab);
function onActiveTabChange(tab: string) {
  session.setActiveTab(tab);
}

registerViewReset(session.resetToDefault);

const focusInstrumentId = ref<string | null>(null);

const { staleCount, thresholdDays } = usePriceStaleness();
function goSyncInstrumentInfo() {
  session.setActiveTab("instruments");
}

const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (instrumentId) => {
    session.setActiveTab("instruments");
    focusInstrumentId.value = instrumentId;
  },
});
onMounted(() => focusParam.consume());

const LEDGER_TAB_URL_VALUE = "detail";

interface LedgerDeepLinkParams {
  tab: string;
  account: string | null;
  instrument: string | null;
}

function readQueryValue(query: LocationQuery, key: string): string | null {
  const raw = query[key];
  const value = Array.isArray(raw) ? raw[0] : raw;
  return typeof value === "string" && value !== "" ? value : null;
}

let consumedDeepLink: LedgerDeepLinkParams | null = null;

function consumeLedgerDeepLink(query: LocationQuery) {
  const tab = readQueryValue(query, "tab");
  if (tab !== LEDGER_TAB_URL_VALUE) return;
  const payload: LedgerDeepLinkParams = {
    tab,
    account: readQueryValue(query, "account"),
    instrument: readQueryValue(query, "instrument"),
  };
  if (
    consumedDeepLink !== null &&
    payload.tab === consumedDeepLink.tab &&
    payload.account === consumedDeepLink.account &&
    payload.instrument === consumedDeepLink.instrument
  ) {
    return;
  }
  consumedDeepLink = payload;
  session.setActiveTab("ledger");
  if (payload.account) session.setDetailAccount(payload.account);
  if (payload.instrument) session.setDetailInstrument(payload.instrument);
}

watch(
  () => route.query,
  (query) => consumeLedgerDeepLink(query),
  { immediate: true },
);
</script>

<template>
  <div class="viewport-stack-16">
    <!-- 价格过期提示（issue #1190）：有过期标的存在时出现，同步后再查即消失
         （价格失效信号驱动重查）。文案含过期数量 + 判定阈值，动作直达标的页签
         的既有「同步标的信息」入口；分工说明（issue #1377）：导向的入口只修
         现价，不修历史——历史归后台补全，避免「点了同步曲线还在」的误解。 -->
    <NAlert
      v-if="staleCount > 0"
      type="warning"
      :show-icon="true"
      data-testid="price-staleness-alert"
    >
      {{ t("investments.staleness.message", { count: staleCount, days: thresholdDays }) }}
      <NButton
        size="tiny"
        type="primary"
        secondary
        style="margin-left: 8px"
        data-testid="price-staleness-go-sync"
        @click="goSyncInstrumentInfo"
      >
        {{ t("investments.staleness.goSync") }}
      </NButton>
      <NText depth="3" data-testid="price-staleness-hint" style="display: block; margin-top: 4px">
        {{ t("investments.staleness.hint") }}
      </NText>
    </NAlert>

    <!-- 价格历史后台补全的静默计数（issue #1375 / ADR-0122）：只读、不可点、
         不占全局忙碌条、终态静默收起——走势曲线的历史由后台任务自行补齐的
         唯一可见面。 -->
    <HistoryBackfillIndicator />

    <NTabs :value="activeTab" type="line" @update:value="onActiveTabChange">
      <!-- 概览页签（spec #1532 / issue #1536）：默认落点与 ESC 复位目标——一进
           投资页就看到可投资资产与两腿拆分。纯只读，取数口径单点在后端
           `investment_overview`（ADR-0131：全页折本位币单值）。 -->
      <NTabPane key="overview" name="overview">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="SpeedometerOutline" />{{ t("investments.tabs.overview") }}</span
          ></template
        >
        <InvestmentOverviewPanel />
      </NTabPane>

      <NTabPane key="pnl" name="pnl" display-directive="show">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="StatsChartOutline" />{{ t("investments.tabs.pnl") }}</span
          ></template
        >
        <RealizedPnlPanel />
      </NTabPane>

      <!-- 持仓页签（issue #901）：原盈亏页顶部的持仓概览卡整体迁入，
           卡内自带同步接缝与价格失效信号订阅，独立挂载即可自洽。 -->
      <NTabPane key="holdings" name="holdings">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="PieChartOutline" />{{ t("investments.tabs.holdings") }}</span
          ></template
        >
        <HoldingsOverview />
      </NTabPane>

      <!-- 明细页签（投资明细页签，ADR-0135 决策 3 / issue #1779 基座）：位置紧随
           持仓页签；五种投资 kind 行按 kind 分形态呈现，消费投资明细命令（服务端
           分页 + 类型筛选）。NTabPane 必须带 key（= 页签名）：预取/保活语义依赖
           组件实例身份（本文件顶部既有红线注释）。 -->
      <NTabPane key="ledger" name="ledger">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="DocumentTextOutline" />{{ t("investments.tabs.ledger") }}</span
          ></template
        >
        <InvestmentLedgerTab />
      </NTabPane>
      <NTabPane key="instruments" name="instruments">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="ListOutline" />{{ t("investments.tabs.instruments") }}</span
          ></template
        >
        <InstrumentBrowser :focus-instrument-id="focusInstrumentId" />
      </NTabPane>
    </NTabs>
  </div>
</template>

<style scoped>
.pane-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
