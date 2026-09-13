import { describe, it, expect } from 'vitest'
import { wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import {
  drawerMenuItemTexts,
  openMobileDrawer,
} from '@ledger/test-support/mobile-nav'
import App from '@/App.vue'
import { routes } from '@/router'
import { useFeatureToggleStore } from '@/stores/feature-toggles'
import { VIEW_STATE_KEYS } from '@/utils/view-state'

// 功能开关的导航层过滤（issue #1242 / ADR-0116 决策 3）：关闭只隐藏入口、
// 不改写收纳清单与侧栏顺序；桌面侧栏与移动抽屉消费同一份菜单构建。

async function mountApp() {
  wireInvokeSeam({ defaults: { get_boot_status: { phase: 'ready', error_code: null } } })
  const router = createRouter({ history: createMemoryHistory(), routes })
  await router.push('/dashboard')
  await router.isReady()
  const wrapper = mount(App, { global: { plugins: [router], stubs: { RouterView: true } } })
  await flushPromises()
  return wrapper
}

function siderItemTexts(wrapper: ReturnType<typeof mount>): string[] {
  return [...wrapper.element.querySelectorAll('.n-layout-sider .n-menu-item-content')].map(
    (el) => el.textContent ?? '',
  )
}

describe('关闭的功能从侧栏与移动抽屉消失（issue #1242 / ADR-0116 决策 3）', () => {
  it('桌面侧栏：关闭预算后入口消失，重开后按自定义顺序回原位置；顺序与收纳存储不变', async () => {
    localStorage.setItem(
      VIEW_STATE_KEYS.sidebarOrder,
      JSON.stringify({ bookkeeping: ['budget', 'transactions', 'accounts'] }),
    )
    const featureToggles = useFeatureToggleStore()
    featureToggles.setFeatureClosed('budget', true)
    const wrapper = await mountApp()

    expect(siderItemTexts(wrapper).some((text) => text.includes('预算'))).toBe(false)
    expect(siderItemTexts(wrapper).some((text) => text.includes('交易'))).toBe(true)

    const storedOrder = localStorage.getItem(VIEW_STATE_KEYS.sidebarOrder)
    const storedContainment = localStorage.getItem(VIEW_STATE_KEYS.sidebarContainment)
    featureToggles.setFeatureClosed('budget', false)
    await flushPromises()

    const reopened = siderItemTexts(wrapper)
    const budgetIndex = reopened.findIndex((text) => text.includes('预算'))
    const transactionsIndex = reopened.findIndex((text) => text.includes('交易'))
    const accountsIndex = reopened.findIndex((text) => text.includes('账户'))
    expect(budgetIndex).toBeGreaterThan(-1)
    expect(budgetIndex).toBeLessThan(transactionsIndex)
    expect(transactionsIndex).toBeLessThan(accountsIndex)
    expect(localStorage.getItem(VIEW_STATE_KEYS.sidebarOrder)).toBe(storedOrder)
    expect(localStorage.getItem(VIEW_STATE_KEYS.sidebarContainment)).toBe(storedContainment)
  })

  it('移动抽屉共用同一构建器：关闭投资后抽屉无投资入口', async () => {
    setFakeMedia({ width: 839 })
    useFeatureToggleStore().setFeatureClosed('investments', true)
    const wrapper = await mountApp()
    await openMobileDrawer(wrapper)

    expect(drawerMenuItemTexts().some((text) => text.includes('投资'))).toBe(false)
    expect(drawerMenuItemTexts().some((text) => text.includes('物品'))).toBe(true)
  })

  it('关闭的主项无键位提示，其固定键空置而后续键位不压缩', async () => {
    useFeatureToggleStore().setFeatureClosed('investments', true)
    const wrapper = await mountApp()
    const sider = wrapper.find('.n-layout-sider')
    expect(sider.text()).not.toContain('⌃4')
    expect(sider.text()).toContain('⌃5')
    expect(sider.text()).toContain('⌃7')
  })

  it('某组「更多」成员全部关闭后链接消失，收纳清单仍原样保留（只过滤入口）', async () => {
    const featureToggles = useFeatureToggleStore()
    for (const id of ['policies', 'physicalAssets', 'insurers'] as const) {
      featureToggles.setFeatureClosed(id, true)
    }
    const wrapper = await mountApp()
    const links = wrapper.findAll('.group-more-link')
    expect(links).toHaveLength(1)
    expect(links[0]!.attributes('title')).toBe('定时、商户')
    expect(localStorage.getItem(VIEW_STATE_KEYS.sidebarContainment)).toBeNull()
  })
})
