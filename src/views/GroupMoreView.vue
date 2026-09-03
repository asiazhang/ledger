<script setup lang="ts">
import { computed, h, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { t } from '@/i18n'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import {
  ShieldCheckmarkOutline,
  CubeOutline,
  RepeatOutline,
  StorefrontOutline,
  SwapHorizontalOutline,
  WalletOutline,
  CalculatorOutline,
  TrendingUpOutline,
  BarChartOutline,
  SearchOutline,
} from '@vicons/ionicons5'
import PoliciesView from '@/views/PoliciesView.vue'
import PhysicalAssetsView from '@/views/PhysicalAssetsView.vue'
import ScheduledView from '@/views/ScheduledView.vue'
import MerchantManager from '@/components/MerchantManager.vue'
import TransactionsView from '@/views/TransactionsView.vue'
import AccountsView from '@/views/AccountsView.vue'
import BudgetView from '@/views/BudgetView.vue'
import InvestmentsView from '@/views/InvestmentsView.vue'
import ItemsView from '@/views/ItemsView.vue'
import ReportsView from '@/views/ReportsView.vue'
import SearchView from '@/views/SearchView.vue'
import { sidebarContainment } from '@/composables/useViewShortcuts'
import type { ContainableViewName, SidebarGroupId } from '@/composables/useViewShortcuts'

/**
 * 组内「更多」聚合页（issue #472 / ADR-0063 决策 1/5）：收纳单位 = 侧栏分组。
 * 页签 = 该组收纳清单序（顺序源模块响应式读出，页签序 = 清单序、成员资格与顺序同源），
 * 默认页签 = 清单首位；页签状态收敛在路由 query.tab（可深链），
 * 切页签 replace 写回（定时页既有约定），无/非法 tab 回默认页签（展示层回退，不写回 query）。
 * 容器零业务逻辑，被收视图整体装载；页签不跨启动持久化（ViewState 只存视图名）。
 * 折叠态不渲染组标题，「更多」链接不出现，本页仅经链接/深链/恢复到达。
 */

const props = defineProps<{ group: SidebarGroupId }>()

/**
 * 收纳成员 → 装配记录（呈现层装配；#473 迁入定时/商户，#474 用户移入主项）：
 * 组件与图标同源一处，键收窄为 ContainableViewName（顺序源模块词表，拼错成员名即编译错误）。
 * 词表含全部主项（#474 移入自由）：任一主项被移入后在此整体装载、功能零损失。
 */
const CONTAINED_VIEWS: Record<ContainableViewName, { component: Component; icon: Component }> = {
  policies: { component: PoliciesView, icon: ShieldCheckmarkOutline },
  physicalAssets: { component: PhysicalAssetsView, icon: CubeOutline },
  // 定时内嵌态：页签退内存态（容器页签与被收视图共用 query.tab 会双写互踩）
  scheduled: { component: () => h(ScheduledView, { embedded: true }), icon: RepeatOutline },
  merchants: { component: MerchantManager, icon: StorefrontOutline },
  // 用户移入的主项（issue #474 / ADR-0063 决策 4：任一主项可入本组「更多」）
  transactions: { component: TransactionsView, icon: SwapHorizontalOutline },
  accounts: { component: AccountsView, icon: WalletOutline },
  budget: { component: BudgetView, icon: CalculatorOutline },
  investments: { component: InvestmentsView, icon: TrendingUpOutline },
  items: { component: ItemsView, icon: CubeOutline },
  reports: { component: ReportsView, icon: BarChartOutline },
  search: { component: SearchView, icon: SearchOutline },
}

const route = useRoute()
const router = useRouter()

/** 该组收纳清单（响应式）：空清单 = 无页签（出厂无成员的预建组）。 */
const tabs = computed(() => sidebarContainment.value[props.group])

/** 页签合法性收窄：query.tab 必须是当前清单成员。 */
function isLegalTab(v: unknown): v is string {
  return typeof v === 'string' && (tabs.value as readonly string[]).includes(v)
}

const activeTab = computed(() => (isLegalTab(route.query.tab) ? route.query.tab : tabs.value[0]))

/** 页签切换走 replace：不产生多余历史记录，深链语义（每页签一条 URL）；
 *  展开既有 query——保留路由上未来可能出现的其他参数。 */
function onTabChange(key: string | number) {
  const tab = String(key)
  if (isLegalTab(tab) && tab !== activeTab.value) {
    void router.replace({ query: { ...route.query, tab } })
  }
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane v-for="name in tabs" :key="name" :name="name">
      <template #tab><span class="pane-tab"><NIcon :component="CONTAINED_VIEWS[name].icon" />{{ t(`common.nav.${name}`) }}</span></template>
      <component :is="CONTAINED_VIEWS[name].component" />
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
