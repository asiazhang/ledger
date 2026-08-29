<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NTabs, NTabPane } from 'naive-ui'
import SubscriptionsPane from '@/components/scheduled/SubscriptionsPane.vue'
import InstallmentsPane from '@/components/scheduled/InstallmentsPane.vue'
import TransfersPane from '@/components/scheduled/TransfersPane.vue'

/**
 * 「定时」统一视图（issue #202）：三页签壳——订阅 / 分期 / 定时转账。
 * 页签状态收敛在路由 query.tab（单字段路由状态，页签可深链/可恢复）；
 * 订阅页签内容整体迁自原 /subscriptions 视图（SubscriptionsPane），
 * 分期页签由 InstallmentsPane 提供（issue #204），
 * 定时转账页签由 TransfersPane 提供（issue #203）。
 */

const TABS = ['subscriptions', 'installments', 'transfers'] as const
type ScheduledTab = (typeof TABS)[number]

const route = useRoute()
const router = useRouter()

/** 页签合法性收窄：TS 无法从 includes 推窄，用类型守卫一处收口。 */
function isScheduledTab(v: unknown): v is ScheduledTab {
  return typeof v === 'string' && (TABS as readonly string[]).includes(v)
}

const activeTab = computed<ScheduledTab>(() =>
  isScheduledTab(route.query.tab) ? route.query.tab : 'subscriptions',
)

/** 页签切换走 replace：不产生多余历史记录，深链语义（每页签一条 URL）；
 *  展开既有 query——保留路由上未来可能出现的其他参数。 */
function onTabChange(key: string | number) {
  const tab = String(key)
  if (isScheduledTab(tab) && tab !== activeTab.value) {
    void router.replace({ query: { ...route.query, tab } })
  }
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane name="subscriptions" tab="订阅">
      <SubscriptionsPane />
    </NTabPane>
    <NTabPane name="installments" tab="分期">
      <InstallmentsPane />
    </NTabPane>
    <NTabPane name="transfers" tab="定时转账">
      <TransfersPane />
    </NTabPane>
  </NTabs>
</template>
