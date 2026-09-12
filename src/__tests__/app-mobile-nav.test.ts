import { describe, it, expect, afterEach, vi } from 'vitest'
import { wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import App from '@/App.vue'
import { routes } from '@/router'
import { useSidebarOrderStore } from '@/stores/sidebar-order'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import {
  openMobileDrawer,
  drawerMenuItemTexts,
  drawerMoreLinkTexts,
  drawerGroupTitles,
  findDrawerItem,
} from '@ledger/test-support/mobile-nav'

/**
 * App 壳按窗口分级分支（issue #842 / ADR-0088 决策 2/4）：经假 matchMedia 换档
 * （spec #838 唯一新接缝），断言两档渲染分支与移动档导航壳行为——
 * ≥840 桌面档零变化（侧栏在、移动壳不在）；<840 移动档抽屉可达全部视图
 * （固定项 + 各组主项 + 各组「更多」页签页），ViewState 组内序/收纳清单
 * 在抽屉中正确渲染且写路径语义零改动。
 */

async function mountApp() {
  // 启动门探测（issue #570 / #601）：桩为明文就绪让主界面照常挂载（sidebar-more-link.test 同款）
  wireInvokeSeam({ defaults: { get_boot_status: { phase: 'ready', error_code: null } } })
  const router = createRouter({ history: createMemoryHistory(), routes })
  await router.push('/dashboard')
  await router.isReady()
  const wrapper = mount(App, { global: { plugins: [router], stubs: { RouterView: true } } })
  await flushPromises()
  return { wrapper, router }
}

describe('App 壳窗口分级分支（issue #842，假 matchMedia 换档）', () => {
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder()
  })

  it('≥840 桌面档零变化：侧栏在、顶栏与抽屉不在，弹层注册表为空；键位提示照渲染', async () => {
    const { wrapper } = await mountApp()
    expect(wrapper.find('.n-layout-sider').exists()).toBe(true)
    expect(wrapper.find('.mobile-top-bar').exists()).toBe(false)
    expect(document.body.querySelector('.n-drawer')).toBeNull()
    expect(hasOpenOverlay()).toBe(false)
    // 桌面档键位带照渲染（ADR-0065）：侧栏菜单项含键位提示（jsdom 平台非 mac，字符为 ⌃）
    const siderText = wrapper.find('.n-layout-sider').text()
    expect(siderText).toContain('⌃1')
    expect(siderText).toContain('⌃,')
  })

  it('缩窗跨断点（1280 → 839）：侧栏卸载、移动顶栏即现（视图名随路由）', async () => {
    const { wrapper } = await mountApp()
    expect(wrapper.find('.n-layout-sider').exists()).toBe(true)
    setFakeMedia({ width: 839 })
    await flushPromises()
    expect(wrapper.find('.n-layout-sider').exists()).toBe(false)
    expect(wrapper.find('.mobile-top-bar').exists()).toBe(true)
    expect(wrapper.find('.mobile-top-bar-title').text()).toBe('概览')
  })

  it('<840 抽屉可达全部视图：固定项 + 各组主项 + 出厂收纳组「更多」链接；洞察组（空清单）无链接', async () => {
    setFakeMedia({ width: 839 })
    const { wrapper } = await mountApp()
    await openMobileDrawer(wrapper)
    expect(hasOpenOverlay()).toBe(true)
    const items = drawerMenuItemTexts().join('|')
    // 固定项（概览/AI/设置）与三组主项（出厂序）
    for (const label of ['概览', '交易', '账户', '预算', '投资', '物品', '报表', '搜索', 'AI', '设置']) {
      expect(items, `抽屉应含 ${label}`).toContain(label)
    }
    // 各组「更多」页签页入口：出厂种子非空组有链接、空清单组无链接（与侧栏同条件）
    expect(drawerMoreLinkTexts()).toEqual(['更多', '更多'])
    expect(drawerGroupTitles()).toEqual(['记账', '资产', '洞察'])
  })

  it('移动档不渲染键位带（ADR-0088 关联 ADR-0065）：抽屉菜单零 ⌘/⌃ 提示，与桌面档同源不同饰', async () => {
    setFakeMedia({ width: 839 })
    const { wrapper } = await mountApp()
    await openMobileDrawer(wrapper)
    for (const text of drawerMenuItemTexts()) {
      expect(text, `抽屉菜单项不应含键位提示：${text}`).not.toMatch(/[⌘⌃]/)
    }
    // 同一份导航状态剥离键位带后仍完整：抽屉菜单与桌面侧栏同构（视图集一致）
    expect(drawerMenuItemTexts()).toHaveLength(10)
  })

  it('ViewState 组内序在抽屉中正确渲染：存量自定义序经解析防御后按序呈现', async () => {
    // 先写存储再挂载：store 启动读路径在首次实例化时消费（单一归宿纪律）
    localStorage.setItem(
      'view_state:sidebar_order',
      JSON.stringify({ bookkeeping: ['budget', 'transactions', 'accounts'] }),
    )
    setFakeMedia({ width: 839 })
    const { wrapper } = await mountApp()
    await openMobileDrawer(wrapper)
    const texts = drawerMenuItemTexts()
    const indexOf = (label: string) => texts.findIndex((t) => t.includes(label))
    expect(indexOf('预算')).toBeGreaterThan(-1)
    expect(indexOf('预算')).toBeLessThan(indexOf('交易'))
    expect(indexOf('交易')).toBeLessThan(indexOf('账户'))
  })

  it('收纳清单写路径语义零改动：移入后抽屉即现该组「更多」链接，移回即消失', async () => {
    setFakeMedia({ width: 839 })
    const { wrapper } = await mountApp()
    await openMobileDrawer(wrapper)
    expect(drawerMoreLinkTexts().length).toBe(2)
    const store = useSidebarOrderStore()
    store.applyMoveIntoMore('search')
    await flushPromises()
    expect(drawerMoreLinkTexts().length).toBe(3)
    store.applyMoveBackToSidebar('search')
    await flushPromises()
    expect(drawerMoreLinkTexts().length).toBe(2)
  })

  it('抽屉菜单项导航：路由切换 + 抽屉关闭（注册表撤销上报）', async () => {
    setFakeMedia({ width: 839 })
    const { wrapper, router } = await mountApp()
    await openMobileDrawer(wrapper)
    findDrawerItem('交易').click()
    // 路由目标是懒加载视图：导航完成需等动态 import（waitFor 轮询而非固定等待）
    await vi.waitFor(() => {
      expect(router.currentRoute.value.name).toBe('transactions')
    })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    expect(wrapper.find('.mobile-top-bar-title').text()).toBe('交易')
  })

  it('抽屉「更多」链接：跳转该组聚合页 + 抽屉关闭（导航即关闭统一收口）', async () => {
    setFakeMedia({ width: 839 })
    const { wrapper, router } = await mountApp()
    await openMobileDrawer(wrapper)
    ;(document.body.querySelector('.n-drawer .group-more-link') as HTMLElement).click()
    await vi.waitFor(() => {
      expect(router.currentRoute.value.name).toBe('bookkeeping-more')
    })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    // 顶栏视图名随「更多」页切换（viewLabel 同源）
    expect(wrapper.find('.mobile-top-bar-title').text()).toBe('记账 · 更多')
  })

  it('开着抽屉跨断点回桌面（839 → 900）：移动壳卸载、注册表兜底撤销、侧栏回归', async () => {
    setFakeMedia({ width: 839 })
    const { wrapper } = await mountApp()
    await openMobileDrawer(wrapper)
    expect(hasOpenOverlay()).toBe(true)
    setFakeMedia({ width: 900 })
    await flushPromises()
    expect(wrapper.find('.mobile-top-bar').exists()).toBe(false)
    expect(wrapper.find('.n-layout-sider').exists()).toBe(true)
    // AppDrawer 卸载兜底：抽屉开着被换档卸载也不滞留开放态（快捷键不被永久抑制）
    expect(hasOpenOverlay()).toBe(false)
  })
})
