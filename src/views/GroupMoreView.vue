<script setup lang="ts">
import { computed, h, nextTick, ref, type Component, type VNode } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { t } from '@/i18n'
import { NTabs, NTabPane, NIcon } from 'naive-ui'
import AppDropdown from '@/components/AppDropdown.vue'
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
  UmbrellaOutline,
} from '@vicons/ionicons5'
import PoliciesView from '@/views/PoliciesView.vue'
import PhysicalAssetsView from '@/views/PhysicalAssetsView.vue'
import ScheduledView from '@/views/ScheduledView.vue'
import MerchantManager from '@/components/MerchantManager.vue'
import InsurerManager from '@/components/InsurerManager.vue'
import TransactionsView from '@/views/TransactionsView.vue'
import AccountsView from '@/views/AccountsView.vue'
import BudgetView from '@/views/BudgetView.vue'
import InvestmentsView from '@/views/InvestmentsView.vue'
import ItemsView from '@/views/ItemsView.vue'
import ReportsView from '@/views/ReportsView.vue'
import SearchView from '@/views/SearchView.vue'
import { useSidebarOrderStore, buildTabContextMenuOptions } from '@/stores/sidebar-order'
import type { ContainableViewName, SidebarGroupId } from '@/stores/sidebar-order'

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
  // 保司管理（issue #714 / ADR-0082 决策 3）：保险域管理面，归位资产组「更多」
  insurers: { component: InsurerManager, icon: UmbrellaOutline },
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

// 顺序状态消费 sidebar-order store（issue #549）：清单/组内序只读，移回写路径经 store。
const sidebarOrder = useSidebarOrderStore()
const { applyMoveBackToSidebar } = sidebarOrder

/** 该组收纳清单（响应式）：空清单 = 无页签（出厂无成员的预建组）。 */
const tabs = computed(() => sidebarOrder.sidebarContainment[props.group])

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

// ---------------------------------------------------------------------------
// 页签右键「移回侧栏」（issue #475 / ADR-0063 决策 4）：手动定位弹出，与侧栏排序菜单
// 同一模式（AppDropdown 封装 + 弹层注册表上报 ADR-0035，零新抑制机制）。
// 组满 3 主项时菜单项置灰并附提示（上限可见、可学习）：提示是两行标签，而 naive
// 菜单项行盒高度固定，两行内容会溢出行盒画到菜单容器外（无背景板漂浮盖内容），
// 故经 render-option 在选项外包一层 .tab-back-option 放开行盒随内容（规则在
// global.css），整颗菜单（项 + 提示）保持一块实心浮层；点选即从清单删除并落本组
// 主项末位（写路径 applyMoveBackToSidebar），页签序随清单响应式更新；
// 移回最后一个成员后清单为空，本页退化为零页签、侧栏「更多」链接随之消失。
// ---------------------------------------------------------------------------

const backMenuShow = ref(false)
const backMenuX = ref(0)
const backMenuY = ref(0)
const backTarget = ref<ContainableViewName | null>(null)

/** 菜单选项由本组当前主项序派生（组满置灰判定在纯函数内，响应式随动）。 */
const backMenuOptions = computed(() => buildTabContextMenuOptions(sidebarOrder.sidebarGroupOrders[props.group]))

/** 右键页签弹出移回菜单：先收起再 nextTick 展开，保证连续弹出时位置刷新（侧栏菜单同款）。 */
function onTabContextmenu(e: MouseEvent, name: ContainableViewName) {
  backTarget.value = name
  backMenuX.value = e.clientX
  backMenuY.value = e.clientY
  backMenuShow.value = false
  void nextTick(() => {
    backMenuShow.value = true
  })
}

function onBackMenuSelect(key: string) {
  backMenuShow.value = false
  const target = backTarget.value
  if (key !== 'backToSidebar' || !target) return
  applyMoveBackToSidebar(target)
}

/** 选项行外包标记类：组满提示两行标签须撑开 naive 固定行盒（规则见 global.css .tab-back-option）。 */
function renderBackOptionRow({ node }: { node: VNode }): VNode {
  return h('div', { class: 'tab-back-option' }, [node])
}
</script>

<template>
  <NTabs type="line" :value="activeTab" @update:value="onTabChange">
    <NTabPane v-for="name in tabs" :key="name" :name="name">
      <template #tab><span class="pane-tab" @contextmenu="onTabContextmenu($event, name)"><NIcon :component="CONTAINED_VIEWS[name].icon" />{{ t(`common.nav.${name}`) }}</span></template>
      <component :is="CONTAINED_VIEWS[name].component" />
    </NTabPane>
  </NTabs>
  <!-- 页签右键「移回侧栏」菜单（issue #475）：手动定位弹出，与侧栏排序菜单同一封装 -->
  <AppDropdown
    trigger="manual"
    placement="bottom-start"
    :show="backMenuShow"
    :x="backMenuX"
    :y="backMenuY"
    :options="backMenuOptions"
    :min-width="140"
    :render-option="renderBackOptionRow"
    @select="onBackMenuSelect"
    @clickoutside="backMenuShow = false"
  />
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
