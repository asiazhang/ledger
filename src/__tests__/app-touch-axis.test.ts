import { describe, it, expect, afterEach } from 'vitest'
import { wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import { setFakeMedia } from './helpers/media-mock'
import App from '@/App.vue'
import { routes } from '@/router'
import { useSidebarOrderStore } from '@/stores/sidebar-order'

/**
 * App 壳触控交互轴（issue #843 / ADR-0088 决策 6，词汇表「输入轴」）：
 * 平板横屏 = 桌面档布局 + 触控轴——宽度轴落桌面档（侧栏在、移动壳不在）时，
 * 输入轴触控下侧栏菜单退役键位提示（⌘/⌃），指针轴行为不变；两轴对比组件测试，
 * 换档一律经媒体查询测试接缝（helpers/media-mock）。
 */

async function mountApp() {
  // 启动门探测（issue #570 / #601）：桩为明文就绪让主界面照常挂载（app-mobile-nav 同款）
  wireInvokeSeam({ defaults: { get_boot_status: { phase: 'ready', error_code: null } } })
  const router = createRouter({ history: createMemoryHistory(), routes })
  await router.push('/dashboard')
  await router.isReady()
  const wrapper = mount(App, { global: { plugins: [router], stubs: { RouterView: true } } })
  await flushPromises()
  return { wrapper, router }
}

describe('App 壳触控交互轴（issue #843，宽度轴桌面档 × 输入轴两态）', () => {
  afterEach(() => {
    useSidebarOrderStore().resetSidebarOrder()
  })

  it('桌面档 + 触控轴（平板横屏）：侧栏照常，键位提示退役（无 ⌘/⌃）', async () => {
    setFakeMedia({ width: 900, hover: 'none', pointer: 'coarse' })
    const { wrapper } = await mountApp()
    expect(wrapper.find('.n-layout-sider').exists()).toBe(true)
    expect(wrapper.find('.mobile-top-bar').exists()).toBe(false)
    const siderText = wrapper.find('.n-layout-sider').text()
    expect(siderText).toContain('概览')
    expect(siderText).not.toMatch(/[⌘⌃]/)
  })

  it('桌面档 + 指针轴：键位提示照渲染（行为不变，两轴对比）', async () => {
    setFakeMedia({ width: 900, hover: 'hover', pointer: 'fine' })
    const { wrapper } = await mountApp()
    const siderText = wrapper.find('.n-layout-sider').text()
    expect(siderText).toContain('⌃1')
    expect(siderText).toContain('⌃,')
  })

  it('运行中换轴实时切换：指针轴渲染提示 → 切触控轴即退役 → 切回即恢复', async () => {
    setFakeMedia({ width: 900, hover: 'hover', pointer: 'fine' })
    const { wrapper } = await mountApp()
    expect(wrapper.find('.n-layout-sider').text()).toMatch(/⌃/)

    setFakeMedia({ hover: 'none', pointer: 'coarse' })
    await flushPromises()
    expect(wrapper.find('.n-layout-sider').text()).not.toMatch(/[⌘⌃]/)

    setFakeMedia({ hover: 'hover', pointer: 'fine' })
    await flushPromises()
    expect(wrapper.find('.n-layout-sider').text()).toMatch(/⌃/)
  })
})
