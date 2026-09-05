import { describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setActivePinia, createPinia } from 'pinia'
import App from '@/App.vue'
import { routes } from '@/router'
import { useSidebarOrderStore, GROUP_CONTAINMENT_SEEDS } from '@/stores/sidebar-order'

// 侧栏组标题行「更多」链接显隐渲染（issue #475 验收末项：移回组内最后一个收纳成员后链接消失；
// #472/#473 建立的渲染条件 = 清单非空，App.vue 消费 sidebarContainment 响应式派生）。
// 容器内测试只覆盖「镜像面」（清单空 → 零页签），此处挂真实 App 侧栏断言链接本体。

enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

async function mountApp() {
  setActivePinia(createPinia())
  const r = createRouter({ history: createMemoryHistory(), routes })
  await r.push('/dashboard')
  await r.isReady()
  // RouterView 打桩：侧栏渲染与本票无关，避免懒加载真实视图拉取数据
  const wrapper = mount(App, { global: { plugins: [r], stubs: { RouterView: true } } })
  await flushPromises()
  return wrapper
}

/** 各组「更多」链接数（出厂种子非空的组各 1 条，空组 0 条） */
function linkCount(wrapper: { findAll: (s: string) => unknown[] }): number {
  return wrapper.findAll('.group-more-link').length
}

describe('侧栏组标题行「更多」链接显隐渲染（issue #475 / ADR-0063 决策 1：清单非空才渲染，移回即消失）', () => {
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder()
    localStorage.clear()
  })

  it('出厂态：记账（定时/商户）与资产（保单/实物资产）两链接渲染，洞察（空清单）无链接', async () => {
    const wrapper = await mountApp()
    const seededGroups = Object.values(GROUP_CONTAINMENT_SEEDS).filter((s) => s.length > 0).length
    expect(linkCount(wrapper)).toBe(seededGroups)
  })

  it('移入空组（洞察）后链接即现；移回该组最后一个收纳成员后链接消失（渲染条件失效）', async () => {
    const wrapper = await mountApp()
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('reports')
    await flushPromises()
    expect(linkCount(wrapper)).toBe(3)
    // 移回组内全部收纳成员：清单回空，链接随之消失（零弹窗、零换出，纯渲染条件）
    store.applyMoveIntoMore('search')
    store.applyMoveBackToSidebar('reports')
    await flushPromises()
    expect(linkCount(wrapper)).toBe(3)
    store.applyMoveBackToSidebar('search')
    await flushPromises()
    expect(linkCount(wrapper)).toBe(2)
  })

  it('双向随动不串组：洞察链接随成员移回消失，出厂满员组的移回拒写（≤3 硬上限）不影响任何链接', async () => {
    const wrapper = await mountApp()
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('reports')
    await flushPromises()
    expect(linkCount(wrapper)).toBe(3)
    store.applyMoveBackToSidebar('reports') // 洞察组最后一个收纳成员移回：链接消失，出厂两链接保持
    await flushPromises()
    expect(linkCount(wrapper)).toBe(2)
    // 记账出厂满员：移回拒写（菜单置灰的第一道防线之下的写路径兑底），渲染面无变化
    store.applyMoveBackToSidebar('scheduled')
    await flushPromises()
    expect(linkCount(wrapper)).toBe(2)
  })
})
