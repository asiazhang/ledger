import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import ScheduledView from '@/views/ScheduledView.vue'
import { routes, router } from '@/router'
import type { SubscriptionSpendOverview } from '@/types'

const mockInvoke = vi.mocked(invoke)

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
  mockInvoke.mockImplementation(((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve([])
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_insurers') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'subscription_spend_overview') return Promise.resolve(emptySpendOverview)
    if (cmd === 'list_scheduled_transactions') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
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
