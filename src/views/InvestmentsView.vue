<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  StatsChartOutline,
  ListOutline,
  PieChartOutline,
  TrendingUpOutline,
} from '@vicons/ionicons5'
import { api } from '@ledger/api'
import { t } from '@ledger/i18n'
import { useFocusParam } from '@/composables/useFocusParam'
import { registerViewReset } from '@/composables/viewResetRegistry'
import { useInvestmentsSessionStore } from '@/stores/investments-session'
import RealizedPnlPanel from '@/components/investments/RealizedPnlPanel.vue'
import HoldingsOverview from '@/components/investments/HoldingsOverview.vue'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import PortfolioTrendPanel from '@/components/investments/PortfolioTrendPanel.vue'
import type { Instrument } from '@ledger/types'

const route = useRoute()

// 投资页会话状态（issue #1192）：当前页签 + 持仓页签筛选/排序/页码 + 走势页签
// 选中标的（模式/预设区间）提升为会话级 store（ADR-0094 会话内保留，先例
// 报表页 #427 / 交易页 #893）——切走页签再切回（或经侧栏离开再回来）回到离开
// 时的样子，数据照常按恢复的选择现拉；冷启动回默认；全程零写盘、不写回 URL。
// 组件仍按 display-directive 'if' 重新挂载（ADR-0094 明确否决 KeepAlive），
// 保留由状态提升承担。
const session = useInvestmentsSessionStore()
const activeTab = computed({
  get: () => session.activeTab,
  set: (tab: string) => {
    session.setActiveTab(tab)
  },
})

// ESC 复位接线（ADR-0094 决策 4）：本视图持有保留态，setup 期向复位回调注册表
// 声明复位回调、作用域销毁时自动撤销（导航离开/跨断点换档卸载均不滞留）；
// 窗口行为守卫在无弹层 ESC 时消费。复位走 store 既有复位出口 resetToDefault
// （页签回默认、持仓筛选三维清零、翻页归零、走势回默认组合曲线），同值幂等。
registerViewReset(session.resetToDefault)

// 走势 tab 的单标的入口（issue #139）：标的列表「走势」按钮带入标的（写会话
// store 并切到走势页签）；走势 tab 保持默认 'if'，每次进入重新挂载，选中标的
// 经 store 恢复——与面板内下拉切换同一事实源。
function onViewTrend(inst: Instrument) {
  session.showTrendInstrument(inst)
  session.setActiveTab('trend')
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
    session.setActiveTab('trend')
    void api.getInstrument(instrumentId).then(
      (inst) => {
        // 异步解析期间用户已离开走势页签则丢弃（同读一次语义的迟到意图）
        if (session.activeTab === 'trend') session.showTrendInstrument(inst)
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
         持仓/标的/走势 tab 保持默认 'if'，切回时重新挂载加载（ADR-0094 否决
         KeepAlive）；持仓与走势的瞬态选择经投资页会话 store 恢复（issue #1192）。 -->
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
      <PortfolioTrendPanel />
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
