import { beforeAll, describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NModal, NSelect } from 'naive-ui'
import {
  createOverlayToken,
  hasOpenOverlay,
  openOverlayNames,
  resetOverlays,
} from '@/composables/overlayRegistry'
import AppSelect from '@/components/AppSelect.vue'
import AppModal from '@/components/AppModal.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'

afterEach(() => resetOverlays())

describe('overlayRegistry 单元语义', () => {
  it('token 上报驱动 hasOpenOverlay，撤销后归零', () => {
    const token = createOverlayToken('select')
    expect(hasOpenOverlay()).toBe(false)
    token.set(true)
    expect(hasOpenOverlay()).toBe(true)
    expect(openOverlayNames()).toEqual(['select'])
    token.set(false)
    expect(hasOpenOverlay()).toBe(false)
  })

  it('resetOverlays 清空全部状态（测试复位用）', () => {
    createOverlayToken('modal').set(true)
    createOverlayToken('select').set(true)
    expect(hasOpenOverlay()).toBe(true)
    resetOverlays()
    expect(hasOpenOverlay()).toBe(false)
  })
})

describe('AppSelect 封装契约：NSelect 的 update:show 驱动注册表', () => {
  it('子组件上报打开/关闭，注册表随之翻转，调用方监听同步收到', async () => {
    const received: boolean[] = []
    const wrapper = mount(AppSelect, {
      props: { options: [], 'onUpdate:show': (v: boolean) => received.push(v) },
    })
    const inner = wrapper.findComponent(NSelect)
    inner.vm.$emit('update:show', true)
    expect(hasOpenOverlay()).toBe(true)
    inner.vm.$emit('update:show', false)
    expect(hasOpenOverlay()).toBe(false)
    // update:show 经 attrs 合并照常到达调用方（v-model:show 兼容）
    expect(received).toEqual([true, false])
  })

  it('受控 show prop 变更同样驱动注册表（attrs watch 兜底）', async () => {
    const wrapper = mount(AppSelect, { props: { options: [], show: false } })
    await wrapper.setProps({ show: true })
    expect(hasOpenOverlay()).toBe(true)
    await wrapper.setProps({ show: false })
    expect(hasOpenOverlay()).toBe(false)
  })

  it('未传 show 的非受控用法不被 Boolean 转型变成受控关闭（回归：菜单必须能打开）', async () => {
    Element.prototype.scrollTo = () => {}
    const wrapper = mount(AppSelect, {
      props: { options: [{ label: 'A', value: 'a' }], virtualScroll: false },
      attachTo: document.body,
    })
    await wrapper.find('.n-base-selection').trigger('click')
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)
    expect(document.querySelector('.n-base-select-menu')).not.toBeNull()
    wrapper.unmount()
  })
})

describe('AppModal 封装契约：show 状态驱动注册表', () => {
  it('受控打开即上报，关闭经 update:show 撤销，调用方监听同步收到', () => {
    const received: boolean[] = []
    const wrapper = mount(AppModal, {
      props: { show: true, 'onUpdate:show': (v: boolean) => received.push(v) },
    })
    expect(hasOpenOverlay()).toBe(true)
    const inner = wrapper.findComponent(NModal)
    inner.vm.$emit('update:show', false)
    expect(hasOpenOverlay()).toBe(false)
    expect(received).toEqual([false])
  })
})

describe('真回归（筛选下拉关闭后快捷键永久失效）：真实 PinyinSelect 开→选→关，注册表必须归零', () => {
  beforeAll(() => {
    // jsdom 未实现 Element.scrollTo，naive-ui 菜单内的 VirtualList 需要它
    Element.prototype.scrollTo = () => {}
  })

  const options = [
    { label: '现金', value: 'cash' },
    { label: '银行卡', value: 'bank' },
  ]

  it('选中选项、菜单关闭后 hasOpenOverlay() 恢复 false（残留 DOM 不再误判）', async () => {
    const wrapper = mount(PinyinSelect, {
      props: { options, virtualScroll: false },
      attachTo: document.body,
    })
    expect(hasOpenOverlay()).toBe(false)

    // 打开菜单 → naive-ui 非受控模式经 update:show 上报
    await wrapper.find('.n-base-selection').trigger('click')
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)

    // 点选选项 → 菜单关闭 → 必须撤销上报（旧实现：菜单 display:none 残留
    // body，存在性嗅探永久判「打开」，两套快捷键自此静默失效）
    // 注意：菜单经 VFollower teleport 到 body，不在 wrapper 子树内，需原生查 DOM
    const option = document.querySelector('.n-base-select-option')
    expect(option).not.toBeNull()
    option!.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)

    wrapper.unmount()
    expect(hasOpenOverlay()).toBe(false)
  })

  it('点击外部关闭菜单同样撤销上报', async () => {
    const wrapper = mount(PinyinSelect, {
      props: { options, virtualScroll: false },
      attachTo: document.body,
    })
    await wrapper.find('.n-base-selection').trigger('click')
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)

    // 点击外部关闭：naive-ui 的 clickoutside 指令监听 document 的 mousedown+mouseup
    for (const type of ['mousedown', 'mouseup']) {
      document.body.dispatchEvent(new MouseEvent(type, { bubbles: true }))
    }
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)

    wrapper.unmount()
  })
})
