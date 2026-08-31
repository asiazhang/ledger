<script setup lang="ts">
import { ref, watch } from 'vue'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  StatsChartOutline,
  ListOutline,
  TrendingUpOutline,
} from '@vicons/ionicons5'
import RealizedPnlPanel from '@/components/investments/RealizedPnlPanel.vue'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import PortfolioTrendPanel from '@/components/investments/PortfolioTrendPanel.vue'
import type { Instrument } from '@/types'

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
</script>

<template>
  <NTabs v-model:value="activeTab" type="line">
    <!-- pnl pane 用 display-directive='show'：内容保持挂载（v-show 隐藏），
         筛选/汇总状态在 tab 切换间保留，与原视图顶层 ref 行为一致。
         标的/走势 tab 保持默认 'if'，切回时重新挂载加载，与原 watch(activeTab) 刷新一致。 -->
    <NTabPane name="pnl" display-directive="show">
      <template #tab><span class="pane-tab"><NIcon :component="StatsChartOutline" />盈亏</span></template>
      <RealizedPnlPanel />
    </NTabPane>

    <NTabPane name="instruments">
      <template #tab><span class="pane-tab"><NIcon :component="ListOutline" />标的</span></template>
      <InstrumentBrowser @view-trend="onViewTrend" />
    </NTabPane>

    <NTabPane name="trend">
      <template #tab><span class="pane-tab"><NIcon :component="TrendingUpOutline" />走势</span></template>
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
