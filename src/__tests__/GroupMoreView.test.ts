import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import GroupMoreView from '@/views/GroupMoreView.vue'
import { makePolicy, makePolicyStats } from './factories'
import { routes, router } from '@/router'
import type { Currency, Merchant } from '@/types'
import type { SubscriptionSpendOverview } from '@/types'

const mockInvoke = vi.mocked(invoke)

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockMerchants: Merchant[] = [
  { id: 'mer-1', name: '平安保险', is_deleted: false, created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z', version: 1, device_id: 'test' },
]

/** 订阅花费总览空数据（定时页签挂载即拉取，容器壳测试不关心行内容）。 */
const emptySpendOverview: SubscriptionSpendOverview = {
  native_currency: 'CNY',
  this_month_native_cents: 0,
  this_year_native_cents: 0,
  months: [],
  rows: [],
  projected_month_native_cents: 0,
  projected_year_native_cents: 0,
}

/** 页签挂载即拉取：给最小空数据（容器壳测试不关心行内容）。 */
function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
    if (cmd === 'list_policies') return Promise.resolve([])
    if (cmd === 'list_scheduled_transactions') return Promise.resolve([])
    if (cmd === 'subscription_spend_overview') return Promise.resolve(emptySpendOverview)
    if (cmd === 'list_physical_assets')
      return Promise.resolve({ assets: [], holding_total_native_cents: 0, native_currency: 'CNY' })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

type GroupMoreId = 'bookkeeping' | 'assets' | 'insights'

/** 容器自身页签文案：取首个 .n-tabs-nav（被收视图可能自带嵌套页签，需排除）。 */
function containerTabs(wrapper: { element: Element }): string[] {
  const nav = wrapper.element.querySelectorAll('.n-tabs-nav')[0]!
  return Array.from(nav.querySelectorAll('.n-tabs-tab')).map((t) => t.textContent!.trim())
}

/** 组内「更多」容器：真实路由表同构 memory router + 按组传 prop。 */
async function mountGroupView(group: GroupMoreId, initialPath?: string) {
  const r = createRouter({ history: createMemoryHistory(), routes })
  await r.push(initialPath ?? `/${group}/more`)
  await r.isReady()
  const wrapper = mount(GroupMoreView, { props: { group }, global: { plugins: [r] } })
  await flushPromises()
  return { wrapper, router: r }
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
})

describe('GroupMoreView 组内「更多」容器（issue #472 / ADR-0063 决策 1/5：页签序 = 收纳清单序）', () => {
  it('资产·更多：保单页签为默认页签（清单首位），实物资产追加在后，保单视图整体装载、建档入口可用', async () => {
    const { wrapper } = await mountGroupView('assets')
    expect(wrapper.findAll('.n-tabs-tab').map((t) => t.text())).toEqual(['保单', '实物资产'])
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })

  it('tab query 深链直达保单页签（/assets/more?tab=policies）', async () => {
    const { wrapper, router: r } = await mountGroupView('assets', '/assets/more?tab=policies')
    expect(r.currentRoute.value.query.tab).toBe('policies')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })

  it('实物资产随域归位资产组「更多」（issue #466 / ADR-0064 合入 main 后的接续归位，ADR-0063 决策 5）：深链直达、视图完整装载', async () => {
    const { wrapper, router: r } = await mountGroupView('assets', '/assets/more?tab=physicalAssets')
    expect(r.currentRoute.value.query.tab).toBe('physicalAssets')
    expect(wrapper.text()).toContain('在持估值合计')
    expect(wrapper.find('[data-testid="physical-asset-new"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(false)
  })

  it('点击「实物资产」页签：路由 query.tab replace 写回且合计卡可见', async () => {
    const { wrapper, router: r } = await mountGroupView('assets')
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '实物资产')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBe('physicalAssets')
    expect(wrapper.text()).toContain('在持估值合计')
  })

  it('非法 tab 回退默认页签（清单首位），展示层回退不写回 query', async () => {
    const { wrapper, router: r } = await mountGroupView('assets', '/assets/more?tab=hack')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
    expect(r.currentRoute.value.query.tab).toBe('hack')
  })

  it('保单视图零功能损失：列表/建档/软删入口/保单视角统计在容器内齐备（整体装载，容器零业务逻辑）', async () => {
    const policy = makePolicy({ id: 'policy-1' })
    const stats = makePolicyStats({
      policy_id: 'policy-1',
      total_paid_native_cents: 600_000,
      total_inflow_native_cents: 50_000,
    })
    mockInvoke.mockImplementation(((cmd: string) => {
      if (cmd === 'list_policies') return Promise.resolve([policy])
      if (cmd === 'list_policy_stats') return Promise.resolve([stats])
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve([])
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    }) as typeof invoke)
    const { wrapper } = await mountGroupView('assets')
    const text = wrapper.text()
    // 列表照常渲染（保司/险种/保单号，同 PoliciesView 既有断言口径）
    expect(text).toContain('重疾险')
    expect(text).toContain('P2026-001')
    // 软删入口在行上可达（确认交互归 PoliciesView 自身测试）
    expect(wrapper.find('[data-testid="policy-delete-policy-1"]').exists()).toBe(true)
    // 保单视角统计：累计已缴 600_000 分 → ¥6000（同 PoliciesView 既有断言口径）
    expect(text).toContain('累计已缴')
    expect(text).toContain('¥6000')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })
})

describe('GroupMoreView 记账组接入（issue #473 / ADR-0063 决策 3：定时、商户迁入，页签序 = 清单序）', () => {
  it('记账·更多：定时页签为默认页签（清单首位），商户追加在后，两页签整体装载', async () => {
    const { wrapper } = await mountGroupView('bookkeeping')
    expect(containerTabs(wrapper)).toEqual(['定时', '商户'])
    // 默认页签 = 定时（内嵌态页签退内存态）：订阅面板完整装载
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(true)
  })

  it('tab query 深链直达商户页签（/bookkeeping/more?tab=merchants），商户管理完整装载', async () => {
    const { wrapper, router: r } = await mountGroupView('bookkeeping', '/bookkeeping/more?tab=merchants')
    expect(r.currentRoute.value.query.tab).toBe('merchants')
    expect(wrapper.text()).toContain('商户列表')
    expect(wrapper.text()).toContain('新增商户')
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(false)
  })

  it('内嵌定时页签不占用容器 query.tab：切内页签仅写内存态，容器页签不被互踩', async () => {
    const { wrapper, router: r } = await mountGroupView('bookkeeping')
    // 点击定时视图内部「分期」页签：不写路由 query（内嵌态内存页签）
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '分期')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBeUndefined()
    expect(wrapper.text()).toContain('分期清单')
    // 容器页签仍停在定时（未被内页签值互踩回默认）
    expect(containerTabs(wrapper)).toEqual(['定时', '商户'])
  })

  it('点击容器「商户」页签：query.tab replace 写回，定时视图卸载', async () => {
    const { wrapper, router: r } = await mountGroupView('bookkeeping')
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '商户')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBe('merchants')
    expect(wrapper.text()).toContain('商户列表')
    expect(wrapper.find('[data-testid="sub-create-open"]').exists()).toBe(false)
  })

  it('洞察组路由预建：出厂无收纳成员，容器渲染空页签不崩（链接不渲染的镜像面）', async () => {
    const ins = await mountGroupView('insights')
    expect(ins.router.currentRoute.value.name).toBe('insights-more')
    expect(ins.wrapper.findAll('.n-tabs-tab').length).toBe(0)
  })
})

describe('全局「更多」退役迁移链（issue #473 / ADR-0063 决策 1/5，#202/#472 重定向先例）', () => {
  it('真实路由表：/more 重定向到记账·更多（默认落清单首位定时页签）', async () => {
    await router.push('/more')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('bookkeeping-more')
    expect(router.currentRoute.value.query.tab).toBeUndefined()
  })

  it('真实路由表：/more?tab=merchants 重定向到记账·更多商户页签', async () => {
    await router.push('/more?tab=merchants')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('bookkeeping-more')
    expect(router.currentRoute.value.query.tab).toBe('merchants')
  })

  it('真实路由表：/more?tab=policies 重定向到资产·更多保单页签', async () => {
    await router.push('/more?tab=policies')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('assets-more')
    expect(router.currentRoute.value.query.tab).toBe('policies')
  })

  it('旧 name 仍可解析（ViewState 存量「more」启动恢复经重定向记录落记账·更多，不回退概览）', () => {
    expect(router.hasRoute('more')).toBe(true)
  })

  it('路由切换后持久化的视图名为 bookkeeping-more（重定向终态名入 ViewState，more 不再回写）', async () => {
    await router.push('/more?tab=merchants')
    await flushPromises()
    expect(localStorage.getItem('view_state:route')).toBe(JSON.stringify('bookkeeping-more'))
  })
})
