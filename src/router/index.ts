import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router'
import { saveRouteName } from '@/utils/view-state'
import { useSidebarOrderStore } from '@/stores/sidebar-order'

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
  },
  {
    path: '/investments',
    name: 'investments',
    component: () => import('@/views/InvestmentsView.vue'),
  },
  {
    // 定时（issue #202）：自 #473 起不再是侧栏主项——主入口为记账组「更多」定时页签
    // （issue #473 / ADR-0063 决策 3）。独立路由保留供 ViewState 存量名解析与旧深链
    // （/subscriptions 重定向先例，issue #202）；侧栏不渲染、无键位。
    path: '/scheduled',
    name: 'scheduled',
    component: () => import('@/views/ScheduledView.vue'),
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
    beforeEnter: (to) =>
      useSidebarOrderStore().isViewContained('policies')
        ? { name: 'assets-more', query: { ...to.query, tab: 'policies' } }
        : true,
  },
  {
    // 商户/实物资产独立路由（issue #475 / ADR-0063 决策 4）：两者出厂为收纳成员
    // （记账·商户、资产·实物资产页签），主入口在各组「更多」页；用户右键「移回侧栏」
    // 后以主项身份入侧栏，侧栏/键位导航按 name 路由——独立路由自本票起必须存在。
    path: '/merchants',
    name: 'merchants',
    component: () => import('@/components/MerchantManager.vue'),
  },
  {
    path: '/physical-assets',
    name: 'physicalAssets',
    component: () => import('@/views/PhysicalAssetsView.vue'),
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
  },
  {
    path: '/assets/more',
    name: 'assets-more',
    component: () => import('@/views/GroupMoreView.vue'),
    props: { group: 'assets' },
  },
  {
    path: '/insights/more',
    name: 'insights-more',
    component: () => import('@/views/GroupMoreView.vue'),
    props: { group: 'insights' },
  },
  {
    path: '/budget',
    name: 'budget',
    component: () => import('@/views/BudgetView.vue'),
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
