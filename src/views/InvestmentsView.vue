<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  StatsChartOutline,
  ListOutline,
  PieChartOutline,
  TrendingUpOutline,
} from '@vicons/ionicons5'
import { api } from '@/api'
import { t } from '@/i18n'
import { useFocusParam } from '@/composables/useFocusParam'
import RealizedPnlPanel from '@/components/investments/RealizedPnlPanel.vue'
import HoldingsOverview from '@/components/investments/HoldingsOverview.vue'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import PortfolioTrendPanel from '@/components/investments/PortfolioTrendPanel.vue'
import type { Instrument } from '@/types'

const route = useRoute()

// 各 tab 内容为独立组件：切换 tab（display-directive='if'）会重新挂载，
// 组件 onMounted 内自行加载数据，无需在此协调刷新。
const activeTab = ref('pnl')

// 走势 tab 的单标的入口（issue #139）：标的列表「走势」按钮带入标的，
// 切到走势 tab 后由面板以单标的模式呈现。走势 tab 保持默认 'if'，
// 每次进入重新挂载，入口标的即时生效。
const trendEntry = ref<Instrument | null>(null)

function onViewTrend(inst: Instrument) {
  trendEntry.value = inst
  activeTab.value = 'trend'
}

// 离开走势 tab 即清空入口标的：直入「走势」tab 回到默认组合曲线，
// 不残留上一次从标的列表带入的单标的模式。
watch(activeTab, (tab) => {
  if (tab !== 'trend') trendEntry.value = null
})

// —— 来源跳转落点（spec #704 / issue #709，词汇表「实体定位参数（focus 参数）」）：
// 挂载消费一次（读一次语义归 useFocusParam 单点）。标的落走势页签——标的浏览
// 有分页、行高亮不可靠，走势页签是唯一焦点面：切页签后按 id 精确解析标的
// （清仓/无持仓照常可达，走势不依赖持仓）再带入单标的模式；解析失败（无效
// focus）停留组合走势（不提供落空的跳转）。主项路由与收纳页签（资产「更多」
// investments 页签）共用本视图实例，route.query 同源，两态一套接线。
const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (instrumentId) => {
    activeTab.value = 'trend'
    void api.getInstrument(instrumentId).then(
      (inst) => {
        // 异步解析期间用户已离开走势页签则丢弃（同读一次语义的迟到意图）
        if (activeTab.value === 'trend') trendEntry.value = inst
      },
      () => {},
    )
  },
})
onMounted(() => focusParam.consume())
</script>

<template>
  <NTabs v-model:value="activeTab" type="line">
    <!-- pnl pane 用 display-directive='show'：内容保持挂载（v-show 隐藏），
         筛选/汇总状态在 tab 切换间保留，与原视图顶层 ref 行为一致。
         持仓/标的/走势 tab 保持默认 'if'，切回时重新挂载加载，与原 watch(activeTab) 刷新一致。 -->
    <NTabPane name="pnl" display-directive="show">
      <template #tab><span class="pane-tab"><NIcon :component="StatsChartOutline" />{{ t('investments.tabs.pnl') }}</span></template>
      <RealizedPnlPanel />
    </NTabPane>

    <!-- 持仓页签（issue #901）：原盈亏页顶部的持仓概览卡整体迁入，
         卡内自带同步接缝与价格失效信号订阅，独立挂载即可自洽。 -->
    <NTabPane name="holdings">
      <template #tab><span class="pane-tab"><NIcon :component="PieChartOutline" />{{ t('investments.tabs.holdings') }}</span></template>
      <HoldingsOverview />
    </NTabPane>

    <NTabPane name="instruments">
      <template #tab><span class="pane-tab"><NIcon :component="ListOutline" />{{ t('investments.tabs.instruments') }}</span></template>
      <InstrumentBrowser @view-trend="onViewTrend" />
    </NTabPane>

    <NTabPane name="trend">
      <template #tab><span class="pane-tab"><NIcon :component="TrendingUpOutline" />{{ t('investments.tabs.trend') }}</span></template>
      <PortfolioTrendPanel :entry-instrument="trendEntry" />
    </NTabPane>
  </NTabs>
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
