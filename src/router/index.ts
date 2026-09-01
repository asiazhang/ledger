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
    // 保单（issue #360 / ADR-0051）：消费型保险合同的静态档案，侧栏「资产」组
    path: '/policies',
    name: 'policies',
    component: () => import('@/views/PoliciesView.vue'),
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
