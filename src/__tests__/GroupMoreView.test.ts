import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { hasOpenOverlay, resetOverlays } from '@/composables/overlayRegistry'
import GroupMoreView from '@/views/GroupMoreView.vue'
import { useSidebarOrderStore } from '@/stores/sidebar-order'
import { makePolicy, makePolicyStats } from './factories'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import { routes, router } from '@/router'
import type { Merchant } from '@/types'
import type { SubscriptionSpendOverview } from '@/types'


enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

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
  stubReferenceInvoke({
    list_merchants: mockMerchants,
    list_policies: [],
    list_scheduled_transactions: [],
    subscription_spend_overview: emptySpendOverview,
    list_physical_assets: { assets: [], holding_total_native_cents: 0, native_currency: 'CNY' },
  })
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
  it('资产·更多：保单页签为默认页签（清单首位），实物资产、保司追加在后，保单视图整体装载、建档入口可用（issue #714：保司页签入资产组）', async () => {
    const { wrapper } = await mountGroupView('assets')
    expect(wrapper.findAll('.n-tabs-tab').map((t) => t.text())).toEqual(['保单', '实物资产', '保险公司'])
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

  it('保司管理进资产组「更多」（issue #714 / ADR-0082 决策 3）：深链直达、管理页完整装载', async () => {
    const { wrapper, router: r } = await mountGroupView('assets', '/assets/more?tab=insurers')
    expect(r.currentRoute.value.query.tab).toBe('insurers')
    expect(wrapper.text()).toContain('保险公司列表')
    expect(wrapper.text()).toContain('新增保险公司')
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
    stubReferenceInvoke({
      list_policies: [policy],
      list_policy_stats: [stats],
      list_merchants: mockMerchants,
    })
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

describe('GroupMoreView 用户移入页签（issue #474 / ADR-0063 决策 4：移入即本组「更多」末位页签）', () => {
  // 移入写路径会改顺序状态与 ViewState 存储：用后即复位，不污染同文件后续 describe
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder()
    localStorage.clear()
  })

  it('移入洞察组的主项（搜索）即刻成为末位页签且整体装载（移入空组链接即现的容器面）', async () => {
    useSidebarOrderStore().applyMoveIntoMore('search')
    const { wrapper } = await mountGroupView('insights', '/insights/more?tab=search')
    expect(containerTabs(wrapper)).toEqual(['搜索'])
    expect((wrapper.find('.n-input input').element as HTMLInputElement).placeholder).toBeTruthy()
  })

  it('移入记账组的主项（交易）追加在出厂页签之后：清单序 = 页签序（出厂在前、移入缀尾）', async () => {
    useSidebarOrderStore().applyMoveIntoMore('transactions')
    const { wrapper } = await mountGroupView('bookkeeping')
    expect(containerTabs(wrapper)).toEqual(['定时', '商户', '交易'])
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

describe('GroupMoreView 页签右键「移回侧栏」（issue #475 / ADR-0063 决策 4）', () => {
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder()
    localStorage.clear()
    resetOverlays()
  })

  function findBackOption(): Element | undefined {
    return Array.from(document.body.querySelectorAll('.n-dropdown-option')).find((el) =>
      el.textContent?.includes('移回侧栏'),
    )
  }

  it('出厂满员（记账组）：右键页签弹「移回侧栏」置灰且提示文案经 i18n 渲染（定时/商户移回置灰的天然验证场景）；菜单经既有弹层封装上报注册表', async () => {
    const { wrapper } = await mountGroupView('bookkeeping')
    await wrapper.findAll('.pane-tab').find((t) => t.text().includes('定时'))!.trigger('contextmenu')
    await flushPromises()
    // AppDropdown 封装 + 弹层注册表上报（ADR-0035），零新抑制机制
    expect(hasOpenOverlay()).toBe(true)
    const option = findBackOption()
    expect(option).toBeDefined()
    // 置灰：naive-ui 的 disabled 修饰类在选项体（后代节点）上
    expect(option!.querySelector('[class*="disabled"]')).not.toBeNull()
    expect(document.body.textContent).toContain('本组主项已满 3 项')
    // 行高自适应包装层在菜单内：缺了它两行提示会溢出 naive 固定行盒，
    // 画到菜单容器外成为无背景板漂浮文字（机制见 global.css .tab-back-option）
    expect(document.body.querySelector('.tab-back-option')).not.toBeNull()
  })

  it('腾位后右键可选：点选「移回侧栏」即从清单删除（页签消失、落本组主项末位）', async () => {
    useSidebarOrderStore().applyMoveIntoMore('transactions') // 先移出一个主项腾位（出厂满员须先换位）
    const { wrapper } = await mountGroupView('bookkeeping')
    await wrapper.findAll('.pane-tab').find((t) => t.text().includes('定时'))!.trigger('contextmenu')
    await flushPromises()
    const option = findBackOption()
    expect(option).toBeDefined()
    expect(option!.querySelector('[class*="disabled"]')).toBeNull()
    const hit = (option!.querySelector('.n-dropdown-option-body') ?? option!) as HTMLElement
    hit.click()
    await flushPromises()
    // 定时页签消失，剩余页签随清单序（商户出厂在前、交易移入缀尾）
    expect(containerTabs(wrapper)).toEqual(['商户', '交易'])
    const store = useSidebarOrderStore()
    expect(store.sidebarContainment.bookkeeping).toEqual(['merchants', 'transactions'])
    expect(store.sidebarGroupOrders.bookkeeping).toEqual(['accounts', 'budget', 'scheduled'])
  })

  it('移回组内最后一个收纳成员后容器零页签（侧栏「更多」链接渲染条件失效的容器面）', async () => {
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('reports')
    store.applyMoveIntoMore('search')
    store.applyMoveBackToSidebar('reports')
    store.applyMoveBackToSidebar('search')
    expect(store.sidebarContainment.insights).toEqual([])
    const { wrapper } = await mountGroupView('insights')
    expect(wrapper.findAll('.n-tabs-tab').length).toBe(0)
  })
})

describe('移回侧栏后的独立路由（issue #475：侧栏/键位按 name 路由，种子须有真页面）', () => {
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder()
    localStorage.clear()
  })

  it('/merchants 与 /physical-assets 独立路由可达（出厂为收纳成员，移回后导航可达）', async () => {
    await router.push('/merchants')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('merchants')
    await router.push('/physical-assets')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('physicalAssets')
  })

  it('/policies 分流：出厂收纳态重定向资产·更多保单页签（#472 重定向先例不变）', async () => {
    await router.push('/policies')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('assets-more')
    expect(router.currentRoute.value.query.tab).toBe('policies')
  })

  it('/policies 分流：移回侧栏后独立渲染保单页（守卫放行，侧栏主项导航可达）', async () => {
    useSidebarOrderStore().applyMoveBackToSidebar('policies')
    await router.push('/policies')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('policies')
  })
})
