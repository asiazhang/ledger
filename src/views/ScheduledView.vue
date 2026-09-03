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

/**
 * 「定时」统一视图（issue #202）：三页签壳——订阅 / 分期 / 定时转账。
 * 页签状态收敛在路由 query.tab（单字段路由状态，页签可深链/可恢复）；
 * 订阅页签内容整体迁自原 /subscriptions 视图（SubscriptionsPane），
 * 分期页签由 InstallmentsPane 提供（issue #204），
 * 定时转账页签由 TransfersPane 提供（issue #203）。
 * 自 #473 起主入口为记账组「更多」定时页签（ADR-0063 决策 3）；独立路由保留供
 * ViewState 存量与旧深链（见 router）。
 */

const TABS = ['subscriptions', 'installments', 'transfers'] as const
type ScheduledTab = (typeof TABS)[number]

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
      <SubscriptionsPane />
    </NTabPane>
    <NTabPane name="installments">
      <template #tab><span class="pane-tab"><NIcon :component="PulseOutline" />{{ t('scheduled.tab.installments') }}</span></template>
      <InstallmentsPane />
    </NTabPane>
    <NTabPane name="transfers">
      <template #tab><span class="pane-tab"><NIcon :component="SyncOutline" />{{ t('scheduled.tab.transfers') }}</span></template>
      <TransfersPane />
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
