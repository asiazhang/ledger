import { describe, expect, it } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { defineComponent, h, nextTick } from 'vue'
import AppPopover from '@/components/AppPopover.vue'
import { hasOpenOverlay, resetOverlays } from '@/composables/overlayRegistry'

// AppPopover 薄封装契约（issue #834 / ADR-0035 接线）：内容透传渲染 + 开/关
// 实时上报弹层注册表（快捷键抑制零新机制）。完整交互行为由消费方组件测试
// （book-sidebar-entry.test.ts）覆盖，此处只钉封装本体契约。

// 受控替身：封装刻意不声明 show prop（attrs 透传契约），经声明 show 的宿主
// 组件以 :show 绑定驱动（setProps 类型安全，AppModal 契约测试同款形态）。
const ControlledHost = defineComponent({
  props: { show: { type: Boolean, default: false } },
  components: { AppPopover },
  setup(props) {
    return () =>
      h(AppPopover, { trigger: 'manual', show: props.show, x: 10, y: 10 }, {
        default: () => h('div', { class: 'app-popover-marker' }, '内容'),
      })
  },
})

describe('AppPopover（薄封装 NPopover + 弹层注册表上报）', () => {
  it('受控 show 置开时内容经 teleport 渲染，并上报弹层注册表；置关后撤销', async () => {
    resetOverlays()
    const wrapper = mount(ControlledHost, { props: { show: false } })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)

    await wrapper.setProps({ show: true })
    await flushPromises()
    await nextTick()
    expect(document.body.querySelector('.app-popover-marker')).not.toBeNull()
    expect(hasOpenOverlay()).toBe(true)

    await wrapper.setProps({ show: false })
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
  })
})
