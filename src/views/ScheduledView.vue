<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { t } from '@/i18n'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  CalendarOutline,
  PulseOutline,
  SyncOutline,
} from '@vicons/ionicons5'
import SubscriptionsPane from '@/components/scheduled/SubscriptionsPane.vue'
import InstallmentsPane from '@/components/scheduled/InstallmentsPane.vue'
import TransfersPane from '@/components/scheduled/TransfersPane.vue'
import { useFocusParam } from '@/composables/useFocusParam'
import type { ScheduledFormTab } from '@/components/source-jump'

/**
 * 「定时」统一视图（issue #202）：三页签壳——订阅 / 分期 / 定时转账。
 * 页签状态收敛在路由 query.tab（单字段路由状态，页签可深链/可恢复）；
 * 订阅页签内容整体迁自原 /subscriptions 视图（SubscriptionsPane），
 * 分期页签由 InstallmentsPane 提供（issue #204），
 * 定时转账页签由 TransfersPane 提供（issue #203）。
 * 自 #473 起主入口为记账组「更多」定时页签（ADR-0063 决策 3）；独立路由保留供
 * ViewState 存量与旧深链（见 router）。
 */

/** 页签词表与来源跳转深模块同源（source-jump ScheduledFormTab，spec #704/#707
 *  收口：深模块产出 scheduledTab 通道、本视图消费同一形态页签闭集）。 */
const TABS: readonly ScheduledFormTab[] = ['subscriptions', 'installments', 'transfers']
type ScheduledTab = ScheduledFormTab

/**
 * embedded：组内「更多」容器内嵌态（issue #473）。容器页签同样占用 query.tab，
 * 本视图内嵌时退为内存态页签（切页签不读写 query），避免同一路由参数双写互踩；
 * 独立路由态（默认）行为不变。
 */
const props = defineProps<{ embedded?: boolean }>()

const route = useRoute()
const router = useRouter()

/** 页签合法性收窄：TS 无法从 includes 推窄，用类型守卫一处收口。 */
function isScheduledTab(v: unknown): v is ScheduledTab {
  return typeof v === 'string' && (TABS as readonly string[]).includes(v)
}

/** 内嵌态页签（内存态，容器内不落 URL）。 */
const localTab = ref<ScheduledTab>('subscriptions')

// 内嵌态落点页签（spec #704 / issue #707）：来源跳转以 scheduledTab 叠加形态
// 页签（容器 query.tab 归容器，issue #473 双写互踩约定）。装配时读一次落定
// 内存页签——独立路由态形态页签由 query.tab 承载（activeTab 直读），不经此处。
if (props.embedded) {
  const landingTab = route.query.scheduledTab
  if (isScheduledTab(landingTab)) localTab.value = landingTab
}

// —— 计划来源落点（spec #704 / issue #707，词汇表「实体定位参数（focus 参数）」）：
// 读一次语义归 useFocusParam 单点；回调只暂存计划 id，经 focus-plan-id prop 交给
// 目标形态页签，页签装配后打开计划详情弹窗（弹窗按 id 独立取数，不受清单状态
// 过滤影响——已取消计划照常可开）。setup 期消费：先于子页签装配，待开 id 在
// 页签挂载前就位。消费后由页签回报清闸，页签切换不复弹；刷新/重进 = 新实例
// 重定位（URL 在场即复现，深链可分享）。
const pendingFocusPlanId = ref<string | null>(null)
const focusParam = useFocusParam({
  query: () => route.query,
  onFocus: (planId) => {
    pendingFocusPlanId.value = planId
  },
})
focusParam.consume()

/** 页签已消费待开计划（回报清闸：prop 置空，切换页签不复开）。 */
function onPlanFocusConsumed() {
  pendingFocusPlanId.value = null
}

const activeTab = computed<ScheduledTab>(() => {
  if (props.embedded) return localTab.value
  return isScheduledTab(route.query.tab) ? route.query.tab : 'subscriptions'
})

/** 页签切换：独立态走 replace（不产生多余历史记录，深链语义，每页签一条 URL，
 *  展开既有 query——保留路由上未来可能出现的其他参数）；内嵌态仅写内存态。 */
function onTabChange(key: string | number) {
  const tab = String(key)
  if (!isScheduledTab(tab) || tab === activeTab.value) return
  if (props.embedded) {
    localTab.value = tab
    return
  }
  void router.replace({ query: { ...route.query, tab } })
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane name="subscriptions">
      <template #tab><span class="pane-tab"><NIcon :component="CalendarOutline" />{{ t('scheduled.tab.subscriptions') }}</span></template>
      <SubscriptionsPane :focus-plan-id="pendingFocusPlanId" @focus-consumed="onPlanFocusConsumed" />
    </NTabPane>
    <NTabPane name="installments">
      <template #tab><span class="pane-tab"><NIcon :component="PulseOutline" />{{ t('scheduled.tab.installments') }}</span></template>
      <InstallmentsPane :focus-plan-id="pendingFocusPlanId" @focus-consumed="onPlanFocusConsumed" />
    </NTabPane>
    <NTabPane name="transfers">
      <template #tab><span class="pane-tab"><NIcon :component="SyncOutline" />{{ t('scheduled.tab.transfers') }}</span></template>
      <TransfersPane :focus-plan-id="pendingFocusPlanId" @focus-consumed="onPlanFocusConsumed" />
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
