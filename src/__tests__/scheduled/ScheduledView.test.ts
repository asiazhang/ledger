import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from '../helpers/invoke-mock'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import ScheduledView from '@/views/ScheduledView.vue'
import { stubReferenceInvoke } from '../helpers/reference-stubs'
import { routes, router } from '@/router'
import type { SubscriptionSpendOverview } from '@/types'


enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

/** 订阅花费总览空数据（子页签挂载即拉取）。 */
const emptySpendOverview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 0,
  this_year_native_cents: 0,
  months: [],
  rows: [],
  projected_month_native_cents: 0,
  projected_year_native_cents: 0,
}

/** 壳层测试只关心页签结构：子页签的 invoke 一律给最小空数据。 */
function baseInvoke() {
  stubReferenceInvoke({
    list_currencies: [],
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
    subscription_spend_overview: emptySpendOverview,
    list_scheduled_transactions: [],
  })
}

/** memory router：与真实路由表同构（routes 单一来源复用，避免双份漂移）。 */
async function makeRouter(initialPath = '/scheduled') {
  const r = createRouter({ history: createMemoryHistory(), routes })
  await r.push(initialPath)
  await r.isReady()
  return r
}

async function mountView(initialPath = '/scheduled') {
  const r = await makeRouter(initialPath)
  const wrapper = mount(ScheduledView, { global: { plugins: [r] } })
  await flushPromises()
  return { wrapper, router: r }
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
})

describe('ScheduledView 「定时」视图三页签（issue #202）', () => {
  it('默认激活订阅页签：无 tab query 时渲染订阅清单内容', async () => {
    const { wrapper } = await mountView()
    // 订阅页签内容来自迁入的订阅页（清单卡片 + 新建按钮）
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('订阅清单')
  })

  it('三个页签可见：订阅 / 分期 / 定时转账', async () => {
    const { wrapper } = await mountView()
    expect(wrapper.text()).toContain('订阅')
    expect(wrapper.text()).toContain('分期')
    expect(wrapper.text()).toContain('定时转账')
  })

  it('点击「分期」页签：路由 query.tab 更新且显示分期面板（issue #204）', async () => {
    const { wrapper, router: r } = await mountView()
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '分期')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBe('installments')
    // 分期页签渲染分期面板（InstallmentsPane），订阅内容不可见
    expect(wrapper.text()).toContain('分期清单')
    expect(wrapper.find('[data-testid="inst-create-open"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(false)
  })

  it('点击「定时转账」页签：路由 query.tab 更新且渲染定时转账内容', async () => {
    const { wrapper, router: r } = await mountView()
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '定时转账')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBe('transfers')
    // 定时转账页签为端到端竖切（issue #203）：显示清单卡片 + 新建入口
    expect(wrapper.text()).toContain('定时转账清单')
    expect(wrapper.find('[data-testid="transfer-create-open"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(false)
  })

  it('tab query 直接导航到对应页签（深链）', async () => {
    const { wrapper } = await mountView('/scheduled?tab=transfers')
    expect(wrapper.text()).toContain('定时转账清单')
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(false)
  })

  it('非法 tab query 回退订阅页签', async () => {
    const { wrapper } = await mountView('/scheduled?tab=hack')
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(true)
  })
})

describe('旧 /subscriptions 入口重定向（issue #202）', () => {
  it('真实路由表：/subscriptions 重定向到定时视图订阅页签', async () => {
    await router.push('/subscriptions')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('scheduled')
    expect(router.currentRoute.value.query.tab).toBe('subscriptions')
  })
})

describe('内嵌态（issue #473：组内「更多」容器装载，页签退内存态）', () => {
  it('内嵌态默认订阅页签，切内页签仅写内存态、不读写路由 query（容器页签占用 query.tab，避免双写互踩）', async () => {
    const r = await makeRouter('/bookkeeping/more')
    const wrapper = mount(ScheduledView, { props: { embedded: true }, global: { plugins: [r] } })
    await flushPromises()
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(true)
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '分期')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBeUndefined()
    expect(wrapper.text()).toContain('分期清单')
  })

  it('独立路由态（默认）不受影响：切内页签仍写回 query.tab', async () => {
    const r = await makeRouter('/scheduled')
    const wrapper = mount(ScheduledView, { global: { plugins: [r] } })
    await flushPromises()
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '分期')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBe('installments')
  })
})

// ---------------------------------------------------------------------------
// 计划来源落点（spec #704 / issue #707，词汇表「来源列」「实体定位参数（focus 参数）」）：
// 来源列点击计划 → 定时视图对应形态页签 + 自动打开计划详情弹窗（弹窗按 id
// 独立取数，不受清单状态过滤影响——已取消计划照常可开）。
// ---------------------------------------------------------------------------

import type { ScheduledTransactionDetail } from '@/types'

/** 已取消订阅计划详情（弹窗取数桩；展示名 = 计划名 = 备注）。 */
function planDetailOf(id: string, overrides: Partial<ScheduledTransactionDetail['core']> = {}): ScheduledTransactionDetail {
  return {
    core: {
      id,
      kind: 'subscription',
      status: 'cancelled',
      account_id: 'acc-1',
      category_id: null,
      amount_cents: 3000,
      currency_code: 'CNY',
      recurrence_type: 'monthly',
      recurrence_interval: 1,
      recurrence_day: null,
      start_date: '2026-02-01',
      note: `计划-${id}`,
      created_at: '2026-01-01T00:00:00Z',
      updated_at: '2026-01-01T00:00:00Z',
      version: 1,
      device_id: 'test',
      is_deleted: false,
      ...overrides,
    },
    extension: { scheduled_transaction_id: id, merchant_id: null, policy_id: null },
    pending_occurrences: [],
    completed_occurrences: 0,
    completed_amount_cents: 0,
    occurrences: [],
  }
}

/** 清单 + 详情命令桩：详情按 id 返回已取消计划（清单状态过滤影响不到弹窗）。 */
function withPlanDetailInvoke() {
  stubReferenceInvoke({
    list_currencies: [],
    list_accounts: [],
    list_categories: [],
    list_merchants: [],
    subscription_spend_overview: emptySpendOverview,
    list_scheduled_transactions: [],
    get_scheduled_transaction_detail: (args) =>
      Promise.resolve(planDetailOf(String(args?.id))),
  })
}

describe('计划来源落点（issue #707）：focus 读一次 → 形态页签 + 计划详情弹窗', () => {
  beforeEach(() => {
    withPlanDetailInvoke()
  })

  it('独立路由：focus + 形态页签（query.tab）落对应页签并自动打开计划详情弹窗', async () => {
    const { wrapper } = await mountView('/scheduled?tab=installments&focus=plan-inst-1')
    // 形态页签正确（分期），订阅页签内容不在场
    expect(wrapper.find('[data-testid="inst-create-open"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(false)
    // 计划详情弹窗按 id 自动打开（不受清单状态过滤影响）
    const detailCalls = mockInvoke.mock.calls.filter((c) => c[0] === 'get_scheduled_transaction_detail')
    expect(detailCalls).toHaveLength(1)
    expect((detailCalls[0] as unknown[])[1]).toEqual({ id: 'plan-inst-1' })
    // 弹窗内容 teleport 到 body（AppModal 先例）：在 body 上查询
    expect(document.body.querySelector('[data-testid="occ-plan-note"]')?.textContent).toBe('计划-plan-inst-1')
  })

  it('已取消计划同样可开：弹窗照常渲染（取消计划无独立列表入口，唯来源可达）', async () => {
    const { wrapper } = await mountView('/scheduled?tab=subscriptions&focus=plan-cancelled')
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(true)
    expect(document.body.querySelector('[data-testid="occ-plan-note"]')?.textContent).toBe('计划-plan-cancelled')
  })

  it('收纳态：容器页签（tab=scheduled）+ scheduledTab 叠加落对应形态页签并打开弹窗', async () => {
    const r = await makeRouter('/bookkeeping/more?tab=scheduled&scheduledTab=transfers&focus=plan-transfer-1')
    const wrapper = mount(ScheduledView, { props: { embedded: true }, global: { plugins: [r] } })
    await flushPromises()
    // 形态页签正确（定时转账），容器 query.tab 不被内嵌页签写回（双写互踩约定）
    expect(wrapper.find('[data-testid="transfer-create-open"]').exists()).toBe(true)
    expect(r.currentRoute.value.query.tab).toBe('scheduled')
    expect((mockInvoke.mock.calls.filter((c) => c[0] === 'get_scheduled_transaction_detail'))[0]?.[1]).toEqual({ id: 'plan-transfer-1' })
    expect(document.body.querySelector('[data-testid="occ-plan-note"]')?.textContent).toBe('计划-plan-transfer-1')
  })

  it('读一次：消费后切换内嵌页签不重开弹窗（focus 残留 query 不复弹）', async () => {
    const r = await makeRouter('/bookkeeping/more?tab=scheduled&scheduledTab=subscriptions&focus=plan-once')
    const wrapper = mount(ScheduledView, { props: { embedded: true }, global: { plugins: [r] } })
    await flushPromises()
    expect(document.body.querySelector('[data-testid="occ-plan-note"]')?.textContent).toBe('计划-plan-once')
    // 切内页签再切回：清单命令新增，详情命令不重放
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '分期')!.trigger('click')
    await flushPromises()
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '订阅')!.trigger('click')
    await flushPromises()
    const detailCalls = mockInvoke.mock.calls.filter((c) => c[0] === 'get_scheduled_transaction_detail')
    expect(detailCalls).toHaveLength(1)
  })

  it('无 focus 空转：正常进入视图不拉详情（unexpected invoke 守卫即证）', async () => {
    mockInvoke.mockReset()
    baseInvoke()
    const { wrapper } = await mountView('/scheduled')
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(true)
    expect(document.body.querySelector('[data-testid="occ-plan-note"]')).toBeNull()
  })
})
