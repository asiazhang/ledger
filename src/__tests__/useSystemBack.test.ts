import { describe, it, expect, afterEach, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent, h, ref } from 'vue'
import { createMemoryHistory, createRouter } from 'vue-router'
import { NDialogProvider } from 'naive-ui'
import { setFakeMedia } from '@ledger/test-support/media-mock'
import {
  captureBackHandler,
  captureBackRegistration,
  mockOnBackButtonPress,
  mockWindowDestroy,
} from '@ledger/test-support/back-mock'
import { useSystemBack } from '@/composables/useSystemBack'
import { createOverlayToken, hasOpenOverlay, openOverlayNames, resetOverlays } from '@/composables/overlayRegistry'
import { useAppDialog } from '@/composables/useAppDialog'
import AppModal from '@/components/AppModal.vue'
import { routes } from '@/router'

/**
 * 系统返回桥接（issue #845 / ADR-0088 决策 7）：返回事件按窗口分级挂载
 * （移动档才注册监听，桌面档零渗透），语义三段——有弹层关最上层（复用弹层
 * 注册表判定与关闭出口）→ 无弹层路由回退 → 栈底交还系统（销毁主窗口）。
 */

afterEach(() => resetOverlays())

async function mountBackHost() {
  const router = createRouter({ history: createMemoryHistory(), routes })
  await router.push('/dashboard')
  await router.isReady()
  const Host = defineComponent({
    setup() {
      useSystemBack()
      return () => h('div', 'host')
    },
  })
  const wrapper = mount(Host, { global: { plugins: [router] } })
  await flushPromises()
  return { wrapper, router }
}

describe('注册面：按窗口分级挂载，桌面档零渗透', () => {
  it('桌面档（默认 1280）不注册返回监听', async () => {
    await mountBackHost()
    expect(mockOnBackButtonPress).not.toHaveBeenCalled()
  })

  it('移动档（<840）注册返回监听', async () => {
    setFakeMedia({ width: 839 })
    await mountBackHost()
    expect(mockOnBackButtonPress).toHaveBeenCalledTimes(1)
  })

  it('跨断点换档（839 → 1280）：撤销已注册监听', async () => {
    setFakeMedia({ width: 839 })
    const read = captureBackRegistration()
    const { wrapper } = await mountBackHost()
    const registration = read()
    expect(registration).not.toBeNull()
    setFakeMedia({ width: 1280 })
    await flushPromises()
    expect(registration!.unregister).toHaveBeenCalled()
    wrapper.unmount()
  })

  it('卸载时撤销注册', async () => {
    setFakeMedia({ width: 839 })
    const read = captureBackRegistration()
    const { wrapper } = await mountBackHost()
    const registration = read()
    expect(registration).not.toBeNull()
    wrapper.unmount()
    await flushPromises()
    expect(registration!.unregister).toHaveBeenCalled()
  })
})

describe('语义三段', () => {
  it('有弹层 → 关最上层：路由不动、不交还系统', async () => {
    setFakeMedia({ width: 839 })
    const destroy = mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const { wrapper, router } = await mountBackHost()
    const requestClose = vi.fn(() => {
      token.set(false)
      return true
    })
    const token = createOverlayToken('modal', requestClose)
    token.set(true)
    await router.push('/transactions')

    triggerBack({ canGoBack: true })

    expect(requestClose).toHaveBeenCalledTimes(1)
    expect(router.currentRoute.value.name).toBe('transactions')
    expect(destroy).not.toHaveBeenCalled()
    expect(hasOpenOverlay()).toBe(false)
    wrapper.unmount()
  })

  it('无弹层 + 可回退 → 路由回退', async () => {
    setFakeMedia({ width: 839 })
    const destroy = mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const { wrapper, router } = await mountBackHost()
    await router.push('/transactions')

    triggerBack({ canGoBack: true })
    await flushPromises()

    expect(router.currentRoute.value.name).toBe('dashboard')
    expect(destroy).not.toHaveBeenCalled()
    wrapper.unmount()
  })

  it('无弹层 + 栈底（canGoBack=false）→ 交还系统（销毁主窗口），路由不动', async () => {
    setFakeMedia({ width: 839 })
    const destroy = mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const { wrapper, router } = await mountBackHost()
    await router.push('/transactions')

    triggerBack({ canGoBack: false })
    await flushPromises()

    expect(destroy).toHaveBeenCalledTimes(1)
    expect(router.currentRoute.value.name).toBe('transactions')
    wrapper.unmount()
  })

  it('栈顶不可关（无 requestClose 通道）→ 吞掉本次返回：不回退、不交还系统', async () => {
    setFakeMedia({ width: 839 })
    const destroy = mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const { wrapper, router } = await mountBackHost()
    const token = createOverlayToken('modal')
    token.set(true)
    await router.push('/transactions')

    triggerBack({ canGoBack: true })
    await flushPromises()

    expect(router.currentRoute.value.name).toBe('transactions')
    expect(destroy).not.toHaveBeenCalled()
    expect(hasOpenOverlay()).toBe(true)
    token.set(false)
    wrapper.unmount()
  })
})

describe('多弹层叠开按序逐层关（验收用例，issue #845）', () => {
  it('弹窗 + 命令式确认框叠开：返回先关确认框再关弹窗，按注册表栈序', async () => {
    setFakeMedia({ width: 839 })
    const destroy = mockWindowDestroy()
    const triggerBack = captureBackHandler()
    const router = createRouter({ history: createMemoryHistory(), routes })
    await router.push('/dashboard')
    await router.isReady()

    const Host = defineComponent({
      setup() {
        useSystemBack()
        const dialog = useAppDialog()
        const showModal = ref(false)
        return () => [
          h(AppModal, { show: showModal.value, 'onUpdate:show': (v: boolean) => (showModal.value = v) }, { default: () => h('div', 'modal-body') }),
          h('button', { class: 'open-modal', onClick: () => (showModal.value = true) }, 'modal'),
          h('button', { class: 'open-dialog', onClick: () => dialog.warning({ title: '确认', content: 'c' }) }, 'dialog'),
        ]
      },
    })
    const wrapper = mount(NDialogProvider, { slots: { default: () => h(Host) }, global: { plugins: [router] } })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)

    await wrapper.find('.open-modal').trigger('click')
    await flushPromises()
    await wrapper.find('.open-dialog').trigger('click')
    await flushPromises()
    // 栈序 = 打开序（z 序）：先弹窗后确认框
    expect(openOverlayNames()).toEqual(['modal', 'dialog'])

    triggerBack({ canGoBack: true })
    await flushPromises()
    expect(openOverlayNames()).toEqual(['modal'])

    triggerBack({ canGoBack: true })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    expect(destroy).not.toHaveBeenCalled()
  })
})
