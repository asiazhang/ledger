import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { defineComponent, ref } from 'vue'
import AppDrawer from '@/components/AppDrawer.vue'
import { hasOpenOverlay, openOverlayNames } from '@/composables/overlayRegistry'

/** 受控宿主：以 show 状态驱动 AppDrawer（生产用法 v-model:show 的等价形态） */
function mountDrawer(initialShow: boolean) {
  const Harness = defineComponent({
    components: { AppDrawer },
    setup() {
      const show = ref(initialShow)
      return { show }
    },
    template: `
      <AppDrawer :show="show" placement="left" :width="280">
        <div class="drawer-marker">内容</div>
      </AppDrawer>
    `,
  })
  const wrapper = mount(Harness)
  // NDrawer 内容传送门到 body，断言走 document.body（AppModal.test 同款）
  const marker = () => document.body.querySelector('.drawer-marker')
  const setOpen = async (value: boolean) => {
    ;(wrapper.vm as unknown as { show: boolean }).show = value
    await Promise.resolve()
  }
  return { wrapper, marker, setOpen }
}

describe('AppDrawer 薄封装（issue #842 / ADR-0035：导航抽屉入弹层注册表）', () => {
  it('show=true 上报打开：注册表含 drawer，hasOpenOverlay 为真（快捷键抑制判定）', async () => {
    const { marker } = mountDrawer(true)
    await Promise.resolve()
    expect(marker()).not.toBeNull()
    expect(hasOpenOverlay()).toBe(true)
    expect(openOverlayNames()).toContain('drawer')
  })

  it('show=false 不上报：注册表为空', async () => {
    mountDrawer(false)
    await Promise.resolve()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('开→关随受控状态实时上报（v-model:show 关闭路径同一判定）', async () => {
    const { setOpen } = mountDrawer(true)
    await Promise.resolve()
    expect(hasOpenOverlay()).toBe(true)
    await setOpen(false)
    await Promise.resolve()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('开着时卸载兜底撤销上报：换档卸载不滞留开放态（快捷键不被永久抑制）', async () => {
    const { wrapper } = mountDrawer(true)
    await Promise.resolve()
    expect(hasOpenOverlay()).toBe(true)
    wrapper.unmount()
    expect(hasOpenOverlay()).toBe(false)
  })
})
