import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { NDialogProvider } from 'naive-ui'
import { defineComponent, h } from 'vue'
import { closeTopOverlay, hasOpenOverlay } from '@ledger/ui-kit/overlayRegistry'
import { useAppDialog } from '@/composables/useAppDialog'

/**
 * useAppDialog 壳侧接线契约（ADR-0035 / issue #845）：命令式对话框经 useAppDialog
 * 打开即上报弹层注册表，closeTopOverlay 的关闭请求经 token 携带的 requestClose
 * 通道走 DialogReactive.destroy()（正常离场路径）。overlayRegistry 本体与其余
 * App* 封装的关闭通道契约随 @ledger/ui-kit 包内测试覆盖（issue #1320 测试跟随
 * 被测包）；本文件只钉 useAppDialog——壳侧注册表消费方——的接线语义，被测对象
 * 在壳（useAppDialog 留壳，ADR-0118 决策 5），故测试留壳侧 app project。
 */
describe('封装关闭通道（closeTopOverlay 消费面，issue #845）', () => {
  it('useAppDialog：closeTopOverlay 走 destroy()（删除确认等命令式对话框）', async () => {
    const Host = defineComponent({
      setup() {
        const dialog = useAppDialog()
        return () => h('button', { onClick: () => dialog.warning({ title: 't', content: 'c' }) }, 'del')
      },
    })
    const wrapper = mount(NDialogProvider, { slots: { default: () => h(Host) } })
    await wrapper.find('button').trigger('click')
    expect(hasOpenOverlay()).toBe(true)

    expect(closeTopOverlay()).toBe(true)
    expect(hasOpenOverlay()).toBe(false)
  })
})
