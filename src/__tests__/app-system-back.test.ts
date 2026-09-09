import { describe, it, expect } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { wireInvokeSeam } from './helpers/invoke-mock'
import { setFakeMedia } from './helpers/media-mock'
import { captureBackHandler, mockWindowDestroy } from './helpers/back-mock'
import { openMobileDrawer } from './helpers/mobile-nav'
import App from '@/App.vue'
import { mount } from '@vue/test-utils'
import { routes } from '@/router'
import { hasOpenOverlay } from '@/composables/overlayRegistry'

/**
 * App 壳系统返回桥接（issue #845 / ADR-0088 决策 7）：真壳链路（App 挂载
 * useSystemBack → onBackButtonPress 到达 → 三段语义）——抽屉经 AppDrawer
 * 受控中继关闭、无弹层路由回退、栈底交还系统、桌面档不注册监听。
 * 单元语义（栈序、不可关吞掉、换档撤销）见 useSystemBack.test.ts。
 */

async function mountApp() {
  // 启动门探测：桩为明文就绪让主界面照常挂载（app-mobile-nav.test 同款）
  wireInvokeSeam({ defaults: { get_boot_status: { phase: 'ready', error_code: null } } })
  const router = createRouter({ history: createMemoryHistory(), routes })
  await router.push('/dashboard')
  await router.isReady()
  const wrapper = mount(App, { global: { plugins: [router], stubs: { RouterView: true } } })
  await flushPromises()
  return { wrapper, router }
}

describe('App 壳系统返回桥接（移动档）', () => {
  it('抽屉开着时返回 → 关抽屉（受控中继），路由不动', async () => {
    setFakeMedia({ width: 839 })
    mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const { wrapper, router } = await mountApp()
    await openMobileDrawer(wrapper)
    expect(hasOpenOverlay()).toBe(true)

    triggerBack({ canGoBack: true })
    await flushPromises()

    expect(hasOpenOverlay()).toBe(false)
    expect(router.currentRoute.value.name).toBe('dashboard')
  })

  it('无弹层返回 → 路由回退（dashboard → transactions → 回 dashboard）', async () => {
    setFakeMedia({ width: 839 })
    mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const { wrapper, router } = await mountApp()
    await router.push('/transactions')
    await flushPromises()

    triggerBack({ canGoBack: true })
    await flushPromises()

    expect(router.currentRoute.value.name).toBe('dashboard')
    wrapper.unmount()
  })

  it('栈底返回（canGoBack=false）→ 交还系统：销毁主窗口，路由不动', async () => {
    setFakeMedia({ width: 839 })
    const destroy = mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const { wrapper, router } = await mountApp()

    triggerBack({ canGoBack: false })
    await flushPromises()

    expect(destroy).toHaveBeenCalledTimes(1)
    expect(router.currentRoute.value.name).toBe('dashboard')
    wrapper.unmount()
  })
})

describe('App 壳桌面档零渗透（issue #845）', () => {
  it('桌面档（≥840）不注册返回监听：返回事件面不存在', async () => {
    const { mockOnBackButtonPress } = await import('./helpers/back-mock')
    await mountApp()
    expect(mockOnBackButtonPress).not.toHaveBeenCalled()
  })
})
