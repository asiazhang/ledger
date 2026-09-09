import { beforeAll, describe, it, expect, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { NModal, NSelect } from 'naive-ui'
import { defineComponent, h } from 'vue'
import {
  closeTopOverlay,
  createOverlayToken,
  hasOpenOverlay,
  openOverlayNames,
  resetOverlays,
} from '@/composables/overlayRegistry'
import AppSelect from '@/components/AppSelect.vue'
import AppModal from '@/components/AppModal.vue'
import AppDropdown from '@/components/AppDropdown.vue'
import PinyinSelect from '@/components/PinyinSelect.vue'
import { NDialogProvider } from 'naive-ui'
import { useAppDialog } from '@/composables/useAppDialog'

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

  it('栈序 = 打开序：openOverlayNames 按打开先后排列，后开者为栈顶', () => {
    const a = createOverlayToken('modal')
    const b = createOverlayToken('select')
    a.set(true)
    b.set(true)
    expect(openOverlayNames()).toEqual(['modal', 'select'])
    b.set(false)
    a.set(false)
  })

  it('乱序关闭：栈中项被移除，其余保持相对序（与 z 序同构）', () => {
    const a = createOverlayToken('modal')
    const b = createOverlayToken('select')
    const c = createOverlayToken('dialog')
    a.set(true)
    b.set(true)
    c.set(true)
    b.set(false)
    expect(openOverlayNames()).toEqual(['modal', 'dialog'])
    c.set(false)
    a.set(false)
  })
})

describe('closeTopOverlay（系统返回桥接的关闭出口，issue #845）', () => {
  afterEach(() => resetOverlays())

  it('空栈返回 false（无弹层 → 交由路由回退/交还系统）', () => {
    expect(closeTopOverlay()).toBe(false)
  })

  it('只关栈顶：先开后开两弹层，一次调用只关后开者，先开者保持', () => {
    const closed: string[] = []
    const a = createOverlayToken('modal', () => {
      closed.push('modal')
      a.set(false)
      return true
    })
    const b = createOverlayToken('select', () => {
      closed.push('select')
      b.set(false)
      return true
    })
    a.set(true)
    b.set(true)
    expect(closeTopOverlay()).toBe(true)
    expect(closed).toEqual(['select'])
    expect(hasOpenOverlay()).toBe(true)
    expect(openOverlayNames()).toEqual(['modal'])
    a.set(false)
  })

  it('栈顶无 requestClose 通道时返回 false 且不动栈（不可关：消费方不回退路由）', () => {
    const a = createOverlayToken('modal')
    a.set(true)
    expect(closeTopOverlay()).toBe(false)
    expect(hasOpenOverlay()).toBe(true)
    a.set(false)
  })

  it('requestClose 的返回值原样上抛（通道拒绝关闭时消费方据此吞掉本次返回）', () => {
    const a = createOverlayToken('dialog', () => false)
    a.set(true)
    expect(closeTopOverlay()).toBe(false)
    a.set(false)
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

  it('受控 show 变更同样驱动注册表（attrs watch 兜底）', async () => {
    // AppSelect 刻意不声明 show（见其头注释）：:show 经 attrs 透传 + watch 兜底。
    // 以带声明的宿主组件按生产方式绑定 :show，宿主 setProps 驱动 attrs 变更。
    const Host = defineComponent({
      props: { show: { type: Boolean, required: true } },
      setup: (props) => () => h(AppSelect, { options: [], show: props.show }),
    })
    const wrapper = mount(Host, { props: { show: false } })
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

describe('封装关闭通道（closeTopOverlay 消费面，issue #845）', () => {
  beforeAll(() => {
    Element.prototype.scrollTo = () => {}
  })

  it('AppSelect 非受控用法：开 → closeTopOverlay 关菜单并撤销上报', async () => {
    const wrapper = mount(AppSelect, {
      props: { options: [{ label: 'A', value: 'a' }], virtualScroll: false },
      attachTo: document.body,
    })
    await wrapper.find('.n-base-selection').trigger('click')
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)
    expect(document.querySelector('.n-base-select-menu')).not.toBeNull()

    expect(closeTopOverlay()).toBe(true)
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    // 菜单 DOM 是懒传送门残留（display:none，ADR-0035 已知语义），关闭判定看注册表
    const menu = document.querySelector('.n-base-select-menu')
    expect(menu?.getAttribute('style')).toContain('display: none')
    wrapper.unmount()
  })

  it('AppModal 受控用法：closeTopOverlay 中继调用方监听器（与 ESC 关闭同一条路径）', async () => {
    const Host = defineComponent({
      props: { show: { type: Boolean, required: true } },
      emits: ['update:show'],
      setup: (props, { emit }) => () =>
        h(AppModal, { show: props.show, 'onUpdate:show': (v: boolean) => emit('update:show', v) }),
    })
    const wrapper = mount(Host, { props: { show: true } })
    expect(hasOpenOverlay()).toBe(true)
    expect(closeTopOverlay()).toBe(true)
    expect(wrapper.emitted('update:show')?.at(-1)).toEqual([false])
    await wrapper.setProps({ show: false })
    expect(hasOpenOverlay()).toBe(false)
  })

  it('AppDropdown 非受控 click 触发：closeTopOverlay 关菜单并撤销上报', async () => {
    const wrapper = mount(AppDropdown, {
      props: { options: [{ label: 'A', key: 'a' }], trigger: 'click' },
      slots: { default: () => h('button', 'open') },
      attachTo: document.body,
    })
    await wrapper.find('button').trigger('click')
    await flushPromises()
    expect(hasOpenOverlay()).toBe(true)

    expect(closeTopOverlay()).toBe(true)
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
    wrapper.unmount()
  })

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

  it('受控但未提供监听器的退化用法不可关：返回 false、注册表保持（消费方吞掉返回键）', async () => {
    const Host = defineComponent({
      props: { show: { type: Boolean, required: true } },
      setup: (props) => () => h(AppModal, { show: props.show }),
    })
    const wrapper = mount(Host, { props: { show: true } })
    expect(hasOpenOverlay()).toBe(true)
    expect(closeTopOverlay()).toBe(false)
    expect(hasOpenOverlay()).toBe(true)
    wrapper.unmount()
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
