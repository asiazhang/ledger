import { describe, it, expect } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import MobileNavShell from '@/components/MobileNavShell.vue'
import { hasOpenOverlay } from '@/composables/overlayRegistry'
import { pressReleaseOn } from '@ledger/test-support/dom'
import { openMobileDrawer, drawerMenuItemTexts, findDrawerItem } from '@ledger/test-support/mobile-nav'

/**
 * 移动档导航壳组件测试（issue #842）：只断言「看到什么、交互后发生什么」——
 * 顶栏渲染、抽屉开合经弹层注册表上报（快捷键抑制判定）、选中事件、导航即关闭。
 * 菜单选项以最小桩传入（与桌面侧栏同一份派生的接线归 App 壳测试）。
 */

async function mountShell() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', redirect: '/dashboard' },
      { path: '/dashboard', name: 'dashboard', component: { template: '<div />' } },
      { path: '/transactions', name: 'transactions', component: { template: '<div />' } },
    ],
  })
  await router.push('/dashboard')
  await router.isReady()
  const wrapper = mount(MobileNavShell, {
    global: { plugins: [router] },
    props: {
      title: '概览',
      menuOptions: [
        { key: 'dashboard', label: '概览' },
        { key: 'transactions', label: '交易' },
      ],
    },
    slots: { default: '<div class="content-marker">内容</div>' },
  })
  await flushPromises()
  return { wrapper, router }
}

describe('MobileNavShell 移动档导航壳（issue #842 / ADR-0088 决策 4）', () => {
  it('顶栏渲染：汉堡按钮（i18n 无障碍标签）+ 当前视图名；内容区渲染默认槽', () => {
    return mountShell().then(({ wrapper }) => {
      const label = wrapper.find('.mobile-hamburger').attributes('aria-label')
      expect(label).toBe('打开导航')
      expect(wrapper.find('.mobile-top-bar-title').text()).toBe('概览')
      expect(wrapper.find('.content-marker').exists()).toBe(true)
    })
  })

  it('抽屉打开经弹层注册表上报（快捷键抑制判定为真）；菜单选项渲染', async () => {
    const { wrapper } = await mountShell()
    expect(hasOpenOverlay()).toBe(false)
    await openMobileDrawer(wrapper)
    expect(hasOpenOverlay()).toBe(true)
    expect(drawerMenuItemTexts().join('|')).toContain('交易')
  })

  it('点遮罩关闭：注册表撤销上报（抑制判定回假）', async () => {
    const { wrapper } = await mountShell()
    await openMobileDrawer(wrapper)
    expect(hasOpenOverlay()).toBe(true)
    await pressReleaseOn('.n-drawer-mask')
    expect(hasOpenOverlay()).toBe(false)
  })

  it('菜单选中：发出 select 事件并关抽屉（路由交调用方，本组件不持导航语义）', async () => {
    const { wrapper } = await mountShell()
    await openMobileDrawer(wrapper)
    findDrawerItem('交易').click()
    await flushPromises()
    expect(wrapper.emitted('select')).toEqual([['transactions']])
    expect(hasOpenOverlay()).toBe(false)
  })

  it('导航即关闭：抽屉开着时路由变化（「更多」链接入口）统一关抽屉', async () => {
    const { wrapper, router } = await mountShell()
    await openMobileDrawer(wrapper)
    expect(hasOpenOverlay()).toBe(true)
    await router.push({ name: 'transactions' })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('点当前视图菜单项：抽屉仍关闭（select 事件照发，路由交调用方裁决）', async () => {
    const { wrapper } = await mountShell()
    await openMobileDrawer(wrapper)
    findDrawerItem('概览').click()
    await flushPromises()
    expect(wrapper.emitted('select')).toEqual([['dashboard']])
    expect(hasOpenOverlay()).toBe(false)
  })
})
