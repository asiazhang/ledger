import {
  createRouter,
  createWebHashHistory,
  type LocationQuery,
  type NavigationGuard,
  type RouteRecordRaw,
  type Router,
} from 'vue-router'
import { getSavedRouteName, saveRouteName } from '@/utils/view-state'
import { useSidebarOrderStore } from '@/stores/sidebar-order'
import {
  isClosableFeature,
  useFeatureToggleStore,
  type ClosableFeatureId,
} from '@/stores/feature-toggles'
import { hasFocusParam } from '@/composables/useFocusParam'

/**
 * 关闭功能的路由守卫（issue #1244 / ADR-0116 决策 4/5，复用保单/保司 beforeEnter 先例）：
 * - 关闭态裸访问 = 入口 → 重定向概览（概览不可关，恒为安全落点，决策 5）；
 * - 关闭态带实体定位参数（focus 参数，词汇表界面状态与交互域） = 引用 → 放行直达功能
 *   自有路由——关闭的功能不进组「更多」页签（ADR-0116 决策 3），对保单/保司这类打开态
 *   才有的收纳分流，关闭态继续转收纳会落进无对应页签的组页、丢焦点（#1242）；
 * - 打开态原样放行（决策不变）：收纳分流交 containment 承载。
 * store 在守卫内解析：路由模块求值先于 pinia 装配，闭包提前持有 store 实例会炸。
 */
function featureRouteGuard(
  view: ClosableFeatureId,
  containment?: { target: 'assets-more' | 'bookkeeping-more' | 'insights-more'; tab: string },
): NavigationGuard {
  return (to) => {
    if (useFeatureToggleStore().isFeatureClosed(view)) {
      return hasFocusParam(to.query) ? true : { name: 'dashboard' }
    }
    if (containment && useSidebarOrderStore().isViewContained(view)) {
      return { name: containment.target, query: { ...to.query, tab: containment.tab } }
    }
    return true
  }
}

/**
 * 组「更多」页守卫（issue #1244 / ADR-0116 决策 3/4）：query.tab 指向已关闭功能时，
 * 该页签已不渲染（关闭的功能不进组「更多」页签，#1242）——改落该功能自有路由，由
 * featureRouteGuard 裁决：裸入口回概览、带实体定位参数的引用（focus）放行。
 * 来源列点击自 #1244 起在关闭态直接落独立路由（source-jump 分流），本守卫兜住其余
 * 产生容器 URL 的路径（存量 `/more?tab=` 深链、书签、手工 URL）。
 * 容器形态页签经 scheduledTab 叠加（内嵌定时页签退内存态，容器 query.tab 归容器，
 * #473 约定）：改道独立路由时归位到定时视图自己的 query.tab，形态不丢。
 */
const groupMoreTabGuard: NavigationGuard = (to) => {
  const tab = to.query.tab
  if (typeof tab !== 'string' || !isClosableFeature(tab)) return true
  if (!useFeatureToggleStore().isFeatureClosed(tab)) return true
  const query: LocationQuery = { ...to.query }
  delete query.tab
  if (tab === 'scheduled' && typeof to.query.scheduledTab === 'string') {
    query.tab = to.query.scheduledTab
  }
  return { name: tab, query }
}

// 导出供测试用同构 memory router 复用，避免路由表双份漂移
export const routes: RouteRecordRaw[] = [
  { path: '/', redirect: '/dashboard' },
  {
    path: '/dashboard',
    name: 'dashboard',
    component: () => import('@/views/DashboardView.vue'),
  },
  {
    path: '/transactions',
    name: 'transactions',
    component: () => import('@/views/TransactionsView.vue'),
  },
  {
    path: '/search',
    name: 'search',
    component: () => import('@/views/SearchView.vue'),
  },
  {
    path: '/accounts',
    name: 'accounts',
    component: () => import('@/views/AccountsView.vue'),
  },
  {
    path: '/reports',
    name: 'reports',
    component: () => import('@/views/ReportsView.vue'),
    beforeEnter: featureRouteGuard('reports'),
  },
  {
    path: '/investments',
    name: 'investments',
    component: () => import('@/views/InvestmentsView.vue'),
    beforeEnter: featureRouteGuard('investments'),
  },
  {
    // 定时（issue #202）：自 #473 起不再是侧栏主项——主入口为记账组「更多」定时页签
    // （issue #473 / ADR-0063 决策 3）。独立路由保留供 ViewState 存量名解析与旧深链
    // （/subscriptions 重定向先例，issue #202）；侧栏不渲染、无键位。
    path: '/scheduled',
    name: 'scheduled',
    component: () => import('@/views/ScheduledView.vue'),
    beforeEnter: featureRouteGuard('scheduled'),
  },
  {
    // 旧订阅入口（issue #202）：重定向到定时视图订阅页签，用户无感知；
    // 保留 name 供 ViewState 兼容——旧记录 'subscriptions' 仍可解析并落到订阅页签
    path: '/subscriptions',
    name: 'subscriptions',
    redirect: { name: 'scheduled', query: { tab: 'subscriptions' } },
  },
  {
    path: '/items',
    name: 'items',
    component: () => import('@/views/ItemsView.vue'),
    beforeEnter: featureRouteGuard('items'),
  },
  {
    // 保单（issue #360 / ADR-0051）：消费型保险合同的静态档案，已迁入「更多」页保单页签
    // （issue #371 / ADR-0055），再按域归位资产组「更多」保单页签（issue #472 / ADR-0063 决策 5）。
    // #475 起按收纳状态分流（beforeEnter 守卫）：仍在收纳清单（出厂态与存量 ViewState 场景）
    // 重定向到资产·更多保单页签（重定向先例不变）；用户右键「移回侧栏」后清单不再含保单，
    // 侧栏主项导航（点击/键位）按 name 路由——独立路由渲染保单页。
    // 重定向透传既有 query（spec #704 / issue #706）：来源列深链 /policies?focus=<id>
    // 在收纳态经此落「更多」页签，focus 不丢、高亮不丢（词汇表 focus 参数「落点尊重组内收纳」）。
    path: '/policies',
    name: 'policies',
    component: () => import('@/views/PoliciesView.vue'),
    beforeEnter: featureRouteGuard('policies', { target: 'assets-more', tab: 'policies' }),
  },
  {
    // 商户/实物资产独立路由（issue #475 / ADR-0063 决策 4）：两者出厂为收纳成员
    // （记账·商户、资产·实物资产页签），主入口在各组「更多」页；用户右键「移回侧栏」
    // 后以主项身份入侧栏，侧栏/键位导航按 name 路由——独立路由自本票起必须存在。
    path: '/merchants',
    name: 'merchants',
    component: () => import('@/components/MerchantManager.vue'),
    beforeEnter: featureRouteGuard('merchants'),
  },
  {
    path: '/physical-assets',
    name: 'physicalAssets',
    component: () => import('@/views/PhysicalAssetsView.vue'),
    beforeEnter: featureRouteGuard('physicalAssets'),
  },
  {
    // 保司管理（issue #714 / ADR-0082 决策 3）：保险域自有字典管理视图，出厂为收纳成员
    // （资产·更多保司页签，组内收纳 ADR-0063）；#475 起按收纳状态分流（/policies 守卫先例）：
    // 仍在收纳清单时重定向到资产·更多保司页签；用户右键「移回侧栏」后以主项身份入侧栏，
    // 侧栏导航按 name 路由——独立路由渲染保司管理页。
    path: '/insurers',
    name: 'insurers',
    component: () => import('@/components/InsurerManager.vue'),
    beforeEnter: featureRouteGuard('insurers', { target: 'assets-more', tab: 'insurers' }),
  },
  {
    // 全局「更多」聚合视图已退役（issue #473 / ADR-0063 决策 1/5）：仅留重定向记录，
    // 承接旧视图名（ViewState 存量 'more' 启动恢复落记账·更多，不回退概览）与旧深链。
    // 迁移链：/more → 记账·更多；/more?tab=merchants → 记账·更多商户页签；
    // /more?tab=policies → 资产·更多保单页签（/policies 重定向先例的延伸）。
    path: '/more',
    name: 'more',
    redirect: (to) =>
      to.query.tab === 'policies'
        ? { name: 'assets-more', query: to.query }
        : { name: 'bookkeeping-more', query: to.query },
  },
  {
    // 组内「更多」聚合页（issue #472 / ADR-0063 决策 1/5：路由镜像侧栏层级），
    // 页签 = 该组收纳清单序（顺序源模块出厂种子），页签态在 query.tab；
    // 本票仅资产组有收纳成员（保单），记账/洞察路由预建、出厂无成员不渲染链接。
    path: '/bookkeeping/more',
    name: 'bookkeeping-more',
    component: () => import('@/views/GroupMoreView.vue'),
    props: { group: 'bookkeeping' },
    beforeEnter: groupMoreTabGuard,
  },
  {
    path: '/assets/more',
    name: 'assets-more',
    component: () => import('@/views/GroupMoreView.vue'),
    props: { group: 'assets' },
    beforeEnter: groupMoreTabGuard,
  },
  {
    path: '/insights/more',
    name: 'insights-more',
    component: () => import('@/views/GroupMoreView.vue'),
    props: { group: 'insights' },
    beforeEnter: groupMoreTabGuard,
  },
  {
    path: '/budget',
    name: 'budget',
    component: () => import('@/views/BudgetView.vue'),
    beforeEnter: featureRouteGuard('budget'),
  },
  {
    path: '/ai',
    name: 'ai',
    component: () => import('@/views/AiPromptView.vue'),
  },
  {
    path: '/settings',
    name: 'settings',
    component: () => import('@/views/SettingsView.vue'),
  },
]

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
})

// 记住当前所在视图，供下次启动恢复（ViewState）。
router.afterEach((to) => {
  if (typeof to.name === 'string') saveRouteName(to.name)
})

/**
 * 启动恢复落点（issue #1244 / ADR-0116 决策 5）：上次视图恰为已关闭功能时回退概览——
 * 概览不可关，恒为安全落点，不需要分级规则；其余（含未知名）原样返回，可用性由
 * 调用方 hasRoute 判定。
 */
export function resolveRestoredViewName(saved: string | null): string | null {
  if (saved !== null && useFeatureToggleStore().isFeatureClosed(saved)) return 'dashboard'
  return saved
}

/**
 * 启动时恢复到上次所在视图：缺失 / 非法 / 已关闭 → 落默认路由（dashboard）。
 * 已关闭功能的裸路由守卫（featureRouteGuard）是同语义的第二道接线——书签、深链等
 * 直接访问走守卫，启动恢复走本函数，两条路径都不让用户落进已关闭功能的空壳。
 */
export async function restoreLastView(router: Router): Promise<void> {
  const saved = resolveRestoredViewName(getSavedRouteName())
  if (saved && router.hasRoute(saved)) {
    await router.replace({ name: saved })
  }
}
