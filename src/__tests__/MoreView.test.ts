import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import MoreView from '@/views/MoreView.vue'
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

/** 保单页签挂载即拉取：给最小空数据（容器壳测试不关心行内容）。 */
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

beforeEach(() => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
})

describe('MoreView 「更多」聚合视图页签容器（issue #371）', () => {
  it('默认渲染保单页签：无 tab query 时保单视图完整可用', async () => {
    const { wrapper } = await mountView()
    expect(wrapper.text()).toContain('保单')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })

  it('tab query 深链直达保单页签', async () => {
    const { wrapper, router: r } = await mountView('/more?tab=policies')
    expect(r.currentRoute.value.query.tab).toBe('policies')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })

  it('非法 tab query 回退默认页签（保单），不产生空白页', async () => {
    const { wrapper } = await mountView('/more?tab=hack')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })

  it('非法 tab 回退是展示层的，不写回 query（与定时页约定一致）', async () => {
    const { router: r } = await mountView('/more?tab=hack')
    expect(r.currentRoute.value.query.tab).toBe('hack')
  })
})

describe('商户页签迁入「更多」（issue #444 / ADR-0055 决策 2 清单追加成员）', () => {
  it('页签顺序：保单在前、商户追加在后，容器零业务逻辑', async () => {
    const { wrapper } = await mountView()
    const tabs = wrapper.findAll('.n-tabs-tab').map((t) => t.text())
    expect(tabs).toEqual(['保单', '商户'])
  })

  it('默认页签仍为保单：商户列表不随默认态渲染内容', async () => {
    const { wrapper } = await mountView()
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
    expect(wrapper.text()).not.toContain('新增商户')
  })

  it('点击「商户」页签：路由 query.tab replace 写回且商户管理完整装载', async () => {
    const { wrapper, router: r } = await mountView()
    await wrapper.findAll('.n-tabs-tab').find((t) => t.text() === '商户')!.trigger('click')
    await flushPromises()
    expect(r.currentRoute.value.query.tab).toBe('merchants')
    expect(wrapper.text()).toContain('新增商户')
    expect(wrapper.text()).toContain('商户列表')
    // 整体迁入：既有功能行为不变——新建表单与行操作入口齐备
    expect(wrapper.find('input[placeholder="商户名称"]').exists()).toBe(true)
  })

  it('tab query 深链直达商户页签（/more?tab=merchants）', async () => {
    const { wrapper } = await mountView('/more?tab=merchants')
    expect(wrapper.text()).toContain('新增商户')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(false)
  })

  it('保单迁入先例不变：/policies 旧路由仍落到「更多」保单页签', async () => {
    const { wrapper } = await mountView('/policies')
    expect(wrapper.find('[data-testid="policy-new"]').exists()).toBe(true)
  })
})

describe('旧保单路由迁移（issue #371，#202 先例）', () => {
  it('真实路由表：/policies 重定向到「更多」并携带 tab: policies', async () => {
    await router.push('/policies')
    await flushPromises()
    expect(router.currentRoute.value.name).toBe('more')
    expect(router.currentRoute.value.query.tab).toBe('policies')
  })

  it('真实路由表：旧 name 仍可解析（ViewState 存量恢复路径不产生未知视图）', () => {
    expect(router.hasRoute('policies')).toBe(true)
  })

  it('路由切换后持久化的视图名恒为 more（非旧保单名）', async () => {
    await router.push('/policies')
    await flushPromises()
    expect(localStorage.getItem('view_state:route')).toBe(JSON.stringify('more'))
  })
})
