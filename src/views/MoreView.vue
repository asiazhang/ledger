<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { t } from '@/i18n'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import { ShieldCheckmarkOutline, StorefrontOutline, CubeOutline } from '@vicons/ionicons5'
import PoliciesView from '@/views/PoliciesView.vue'
import PhysicalAssetsView from '@/views/PhysicalAssetsView.vue'
import MerchantManager from '@/components/MerchantManager.vue'

/**
 * 「更多」聚合视图（issue #371 / ADR-0055）：低频视图的单一收容器。
 * 页签容器形态（先例：定时页三页签），低频视图组件整体作为页签装载，容器零业务逻辑；
 * 页签状态收敛在路由 query.tab（单字段路由状态，可深链/可恢复），
 * 切页签 replace 写回（定时页既有约定）；页签合法性守卫，无/非法 tab 回默认页签。
 * 商户管理迁入为第二个页签（issue #444 / ADR-0055 决策 2 清单追加成员），
 * 页签顺序：保单在前且默认不变、商户追加在后；实物资产为第三个页签
 * （issue #466 / spec #465，入口收纳在「更多」聚合页新页签）；
 * 页签切换不触碰抑制语义。
 */

const TABS = ['policies', 'merchants', 'physicalAssets'] as const
type MoreTab = (typeof TABS)[number]

const route = useRoute()
const router = useRouter()

/** 页签合法性收窄：TS 无法从 includes 推窄，用类型守卫一处收口。 */
function isMoreTab(v: unknown): v is MoreTab {
  return typeof v === 'string' && (TABS as readonly string[]).includes(v)
}

const activeTab = computed<MoreTab>(() =>
  isMoreTab(route.query.tab) ? route.query.tab : 'policies',
)

/** 页签切换走 replace：不产生多余历史记录，深链语义（每页签一条 URL）；
 *  展开既有 query——保留路由上未来可能出现的其他参数。 */
function onTabChange(key: string | number) {
  const tab = String(key)
  if (isMoreTab(tab) && tab !== activeTab.value) {
    void router.replace({ query: { ...route.query, tab } })
  }
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane name="policies">
      <template #tab><span class="pane-tab"><NIcon :component="ShieldCheckmarkOutline" />{{ t('common.nav.policies') }}</span></template>
      <PoliciesView />
    </NTabPane>

    <NTabPane name="merchants">
      <template #tab><span class="pane-tab"><NIcon :component="StorefrontOutline" />{{ t('common.nav.merchants') }}</span></template>
      <MerchantManager />
    </NTabPane>

    <NTabPane name="physicalAssets">
      <template #tab><span class="pane-tab"><NIcon :component="CubeOutline" />{{ t('common.nav.physicalAssets') }}</span></template>
      <PhysicalAssetsView />
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
