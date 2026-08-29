<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { NEmpty, NTabs, NTabPane } from 'naive-ui'
import SubscriptionsPane from '@/components/scheduled/SubscriptionsPane.vue'

/**
 * 「定时」统一视图（issue #202）：三页签壳——订阅 / 分期 / 定时转账。
 * 页签状态收敛在路由 query.tab（单字段路由状态，页签可深链/可恢复）；
 * 订阅页签内容整体迁自原 /subscriptions 视图（SubscriptionsPane），
 * 分期与定时转账页签为占位，由后续 issue（#203 / #204）填充。
 */

const TABS = ['subscriptions', 'installments', 'transfers'] as const
type ScheduledTab = (typeof TABS)[number]

const route = useRoute()
const router = useRouter()

const activeTab = computed<ScheduledTab>(() => {
  const tab = route.query.tab
  return typeof tab === 'string' && (TABS as readonly string[]).includes(tab)
    ? (tab as ScheduledTab)
    : 'subscriptions'
})

/** 页签切换走 replace：不产生多余历史记录，深链语义（每页签一条 URL）。 */
function onTabChange(key: string) {
  if (key !== activeTab.value) {
    void router.replace({ query: { tab: key } })
  }
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane name="subscriptions" tab="订阅">
      <SubscriptionsPane />
    </NTabPane>
    <NTabPane name="installments" tab="分期">
      <!-- 占位：分期管理由 issue #203 填充 -->
      <NEmpty description="分期管理建设中" size="large" />
    </NTabPane>
    <NTabPane name="transfers" tab="定时转账">
      <!-- 占位：定时转账由 issue #204 填充 -->
      <NEmpty description="定时转账建设中" size="large" />
    </NTabPane>
  </NTabs>
</template>
