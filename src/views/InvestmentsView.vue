<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { NAlert, NButton, NIcon, NSpace, NTabPane, NTabs, NText } from "naive-ui";
import {
  SpeedometerOutline,
  StatsChartOutline,
  ListOutline,
  DocumentTextOutline,
  PieChartOutline,
  TrendingUpOutline,
} from "@vicons/ionicons5";
import { api } from "@ledger/api";
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
import PortfolioTrendPanel from "@/investment/PortfolioTrendPanel.vue";
import type { Instrument } from "@ledger/types";

const route = useRoute();

// 投资页会话状态（issue #1192）：当前页签 + 持仓页签筛选/排序/页码 + 走势页签
// 选中标的（模式/预设区间）提升为会话级 store（ADR-0094 会话内保留，先例
// 报表页 #427 / 交易页 #893）——切走页签再切回（或经侧栏离开再回来）回到离开
// 时的样子，数据照常按恢复的选择现拉；冷启动回默认；全程零写盘、不写回 URL。
// 组件仍按 display-directive 'if' 重新挂载（ADR-0094 明确否决 KeepAlive），
// 保留由状态提升承担。
const session = useInvestmentsSessionStore();
// 页签受控桥接：`:value` 只读投影 + `@update:value` 直调意图入口（单一写路；
// 不把 store 状态暴露成组件可直写的 ref，ADR-0094「store 是唯一读写方」）。
const activeTab = computed(() => session.activeTab);
function onActiveTabChange(tab: string) {
  session.setActiveTab(tab);
}

// ESC 复位接线（ADR-0094 决策 4）：本视图持有保留态，setup 期向复位回调注册表
// 声明复位回调、作用域销毁时自动撤销（导航离开/跨断点换档卸载均不滞留）；
// 窗口行为守卫在无弹层 ESC 时消费。复位走 store 既有复位出口 resetToDefault
// （页签回默认、持仓筛选三维清零、翻页归零、走势回默认组合曲线），同值幂等。
registerViewReset(session.resetToDefault);

// 走势 tab 的单标的入口（issue #139）：标的列表「走势」按钮带入标的（写会话
// store 并切到走势页签）；走势 tab 保持默认 'if'，每次进入重新挂载，选中标的
// 经 store 恢复——与面板内下拉切换同一事实源。
function onViewTrend(inst: Instrument) {
  session.showTrendInstrument(inst);
  session.setActiveTab("trend");
}

// 价格过期提示（issue #1190）：打开投资页时做一次本地水位检查（零网络请求），
// 有过期（或持仓缺现价）就提示并导向既有「同步标的信息」入口——刻意不做自动
// 同步与定时轮询（ADR-0015 / ADR-0095 的显式触发口径保留，开页面不变成网络
// 操作）。判定与阈值归后端投资域单点，本视图只渲染计数与阈值。
// 「去同步」按钮切到标的页签：那里就是既有同步入口（按钮 + 进度条 + 结果
// 消息复用同一 useInstrumentInfoSync 接缝），不新增第二套同步触发。
const { staleCount, thresholdDays } = usePriceStaleness();
function goSyncInstrumentInfo() {
  session.setActiveTab("instruments");
}

// —— 来源跳转落点（spec #704 / issue #709，词汇表「实体定位参数（focus 参数）」）：
// 挂载消费一次（读一次语义归 useFocusParam 单点）。标的落走势页签——标的浏览
// 有分页、行高亮不可靠，走势页签是唯一焦点面：切页签后按 id 精确解析标的
// （清仓/无持仓照常可达，走势不依赖持仓）再带入单标的模式；解析失败（无效
// focus）停留组合走势（不提供落空的跳转）。主项路由与收纳页签（资产「更多」
// investments 页签）共用本视图实例，route.query 同源，两态一套接线。
const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (instrumentId) => {
    session.setActiveTab("trend");
    void api.getInstrument(instrumentId).then(
      (inst) => {
        // 异步解析期间用户已离开走势页签则丢弃（同读一次语义的迟到意图）
        if (session.activeTab === "trend") session.showTrendInstrument(inst);
      },
      () => {},
    );
  },
});
onMounted(() => focusParam.consume());
</script>

<template>
  <NSpace vertical :size="16">
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

      <!-- pnl pane 用 display-directive='show'：内容保持挂载（v-show 隐藏），
           筛选/汇总状态在 tab 切换间保留，与原视图顶层 ref 行为一致。
           持仓/标的/走势 tab 保持默认 'if'，切回时重新挂载加载（ADR-0094 否决
           KeepAlive）；持仓与走势的瞬态选择经投资页会话 store 恢复（issue #1192）。
           全部 NTabPane 必须带 key（= 页签名）：naive-ui 渲染的 pane 子节点若无
           key，Vue 按位置就地复用同类型组件实例——概览（模板首位）激活时热挂的
           盈亏 pane 在位置 1，切到盈亏后位置 0 由概览 pane 就地改造成盈亏 pane
           （插槽换血 = RealizedPnlPanel 重新挂载重新取数），热实例反而被销毁。
           带 key 后按实例身份比对，'show' 的预取与状态保留语义才真正成立
           （设置页 SettingsView 全量带 key 是既有实践）。 -->
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
        <InstrumentBrowser @view-trend="onViewTrend" />
      </NTabPane>

      <NTabPane key="trend" name="trend">
        <template #tab
          ><span class="pane-tab"
            ><NIcon :component="TrendingUpOutline" />{{ t("investments.tabs.trend") }}</span
          ></template
        >
        <PortfolioTrendPanel />
      </NTabPane>
    </NTabs>
  </NSpace>
</template>

<style scoped>
/* 页签图标 + 文字：gap 负责间距，文字与图标间不落空白，
   保证测试/无障碍按文本定位页签时拿到纯标签文字 */
.pane-tab {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
</style>
