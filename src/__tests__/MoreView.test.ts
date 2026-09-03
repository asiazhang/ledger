import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import MoreView from '@/views/MoreView.vue'
import GroupMoreView from '@/views/GroupMoreView.vue'
import { makePolicy, makePolicyStats } from './factories'
import { routes, router } from '@/router'
import type { Currency, Merchant } from '@/types'

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

/** 页签挂载即拉取：给最小空数据（容器壳测试不关心行内容）。 */
function baseInvoke() {
  mockInvoke.mockImplementation(((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_merchants') return Promise.resolve(mockMerchants)
    if (cmd === 'list_policies') return Promise.resolve([])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }) as typeof invoke)
}

/** memory router：与真实路由表同构（routes 单一来源复用，避免双份漂移）。 */
async function makeRouter(initialPath = '/more') {
  const r = createRouter({ history: createMemoryHistory(), routes })
  await r.push(initialPath)
  await r.isReady()
  return r
}

async function mountView(initialPath = '/more') {
  const r = await makeRouter(initialPath)
  const wrapper = mount(MoreView, { global: { plugins: [r] } })
  await flushPromises()
  return { wrapper, router: r }
}

type GroupMoreId = 'bookkeeping' | 'assets' | 'insights'

/** 组内「更多」容器：真实路由表同构 memory router + 按组传 prop。 */
async function mountGroupView(group: GroupMoreId, initialPath?: string) {
  const r = await makeRouter(initialPath ?? `/${group}/more`)
  const wrapper = mount(GroupMoreView, { props: { group }, global: { plugins: [r] } })
  await flushPromises()
  return { wrapper, router: r }
}

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
})

describe('MoreView 全局「更多」仅剩商户页签（issue #472 / ADR-0063：保单迁出至资产组，全局收容器 #473 退役）', () => {
  it('页签仅剩商户：保单页签已迁出全局「更多」', async () => {
    const { wrapper } = await mountView()
    const tabs = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(tabs).toEqual(['商户'])
  })

  it('默认页签为商户：无 tab query 时商户管理完整装载', async () => {
    const { wrapper } = await mountView()
    expect(wrapper.text()).toContain('商户列表')
    expect(wrapper.text()).toContain('新增商户')
    expect(wrapper.find('input[placeholder="商户名称"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(false)
  })

  it('tab query 深链直达商户页签（/more?tab=merchants）', async () => {
    const { wrapper, router: r } = await mountView('/more?tab=merchants')
    expect(r.currentRoute.value.query.tab).toBe('merchants')
    expect(wrapper.text()).toContain('新增商户')
  })

  it('已迁出的保单页签深链回退默认页签：展示层回退，不写回 query（与定时页约定一致；/more?tab=policies 的重定向属 #473 迁移链，本票不处理）', async () => {
    const { wrapper, router: r } = await mountView('/more?tab=policies')
    expect(wrapper.text()).toContain('商户列表')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(false)
    expect(r.currentRoute.value.query.tab).toBe('policies')
  })
})

describe('GroupMoreView 组内「更多」容器（issue #472 / ADR-0063 决策 1/5：页签序 = 收纳清单序）', () => {
  it('资产·更多：保单页签为默认页签（清单首位），保单视图整体装载、建档入口可用', async () => {
    const { wrapper } = await mountGroupView('assets')
    expect(wrapper.findAll('.n-tabs-tab').map((t) => t.text())).toEqual(['保单'])
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })

  it('tab query 深链直达保单页签（/assets/more?tab=policies）', async () => {
    const { wrapper, router: r } = await mountGroupView('assets', '/assets/more?tab=policies')
    expect(r.currentRoute.value.query.tab).toBe('policies')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
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

  it('记账/洞察组路由预建：出厂无收纳成员，容器渲染空页签不崩', async () => {
    const bk = await mountGroupView('bookkeeping')
    expect(bk.router.currentRoute.value.name).toBe('bookkeeping-more')
    expect(bk.wrapper.findAll('.n-tabs-tab').length).toBe(0)
    const ins = await mountGroupView('insights')
    expect(ins.router.currentRoute.value.name).toBe('insights-more')
    expect(ins.wrapper.findAll('.n-tabs-tab').length).toBe(0)
  })
})

describe('旧保单路由迁移（issue #472 / ADR-0063 决策 5，#202 先例）', () => {
  it('真实路由表：/policies 重定向到资产·更多并携带 tab: policies', async () => {
    await router.push('/policies')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('assets-more')
    expect(router.currentRoute.value.query.tab).toBe('policies')
  })

  it('真实路由表：旧 name 仍可解析（ViewState 存量恢复路径不产生未知视图）', () => {
    expect(router.hasRoute('policies')).toBe(true)
  })

  it('路由切换后持久化的视图名为 assets-more（组内「更多」路由名入 ViewState）', async () => {
    await router.push('/policies')
    await flushPromises()
    expect(localStorage.getItem('view_state:route')).toBe(JSON.stringify('assets-more'))
  })

  it('全局「更多」本票保留：旧持久化视图名 more 仍可解析（不产生未知视图）', () => {
    expect(router.hasRoute('more')).toBe(true)
  })
})
