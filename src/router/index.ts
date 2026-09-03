import { createRouter, createWebHashHistory, type RouteRecordRaw } from 'vue-router'
import { saveRouteName } from '@/utils/view-state'

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
    // 旧路由保留 name 供 ViewState 兼容——存量记录 'policies' 仍可解析并落到资产·更多保单页签。
    path: '/policies',
    name: 'policies',
    redirect: { name: 'assets-more', query: { tab: 'policies' } },
  },
  {
    // 「更多」聚合视图（issue #371）：低频视图的单一收容器，页签态在 query.tab。
    // issue #472 / ADR-0063：保单按域归位资产组后仅剩商户页签；全局收容器待 #473 退役。
    path: '/more',
    name: 'more',
    component: () => import('@/views/MoreView.vue'),
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
