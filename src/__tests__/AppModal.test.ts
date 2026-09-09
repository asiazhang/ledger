import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent, nextTick, ref } from 'vue'
import AppModal from '@/components/AppModal.vue'
import AppDangerConfirmModal from '@/components/AppDangerConfirmModal.vue'
import { MOBILE_CARD_CLASS } from '@/components/app-modal.css.ts'
import { setFakeMedia } from './helpers/media-mock'
import { hasOpenOverlay, openOverlayNames, resetOverlays } from '@/composables/overlayRegistry'

/** 在 body 上查找遮罩元素（弹层抑制同款信号，见 useViewShortcuts）。 */
function findMask(): HTMLElement {
  const el = document.body.querySelector('.n-modal-mask')
  expect(el, '.n-modal-mask 应存在').not.toBeNull()
  return el as HTMLElement
}

/**
 * 遮罩「按下-抬起」完整事件序列：真实浏览器中按下-抬起合成 click，
 * jsdom 不自动合成，手动派发三段事件等价模拟。
 */
async function pressReleaseOnMask(): Promise<void> {
  const mask = findMask()
  mask.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  mask.dispatchEvent(new MouseEvent('mouseup', { bubbles: true }))
  mask.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  await flushPromises()
}

/** 取 body 上已渲染的卡片元素（preset="card" 的可见输出）。 */
function findCard(): HTMLElement {
  const el = document.body.querySelector('.n-card')
  expect(el, '.n-card 应存在').not.toBeNull()
  return el as HTMLElement
}

// naive-ui 的 doUpdateShow 直接调用 onUpdateShow prop 而非 $emit，
// 断言走监听 spy，不用 wrapper.emitted()。
function mountModal(extraProps: Record<string, unknown> = {}) {
  const onUpdateShow = vi.fn()
  const wrapper = mount(AppModal, {
    props: {
      show: true,
      title: '契约弹窗',
      preset: 'card',
      'onUpdate:show': onUpdateShow,
      ...extraProps,
    },
    slots: { default: '<div class="app-modal-marker">内容</div>' },
  })
  return { wrapper, onUpdateShow }
}

describe('AppModal（issue #251 弹层关闭语义收口）', () => {
  it('默认点遮罩不关闭：遮罩「按下-抬起」事件序列后不触发 update:show', async () => {
    const { onUpdateShow } = mountModal()
    await flushPromises()

    // 默认槽透传：内容正常渲染
    expect(document.body.querySelector('.app-modal-marker')).not.toBeNull()

    await pressReleaseOnMask()
    expect(onUpdateShow).not.toHaveBeenCalled()
  })

  it('mask-closable 透传为 true 时遮罩点击照常关闭（显式逃逸门）', async () => {
    const { onUpdateShow } = mountModal({ maskClosable: true })
    await flushPromises()

    await pressReleaseOnMask()
    expect(onUpdateShow).toHaveBeenCalledWith(false)
  })

  it('✕ 关闭路径照常触发关闭（closable 透传）', async () => {
    const { onUpdateShow } = mountModal({ closable: true })
    await flushPromises()

    const closeBtn = document.body.querySelector('.n-base-close')
    expect(closeBtn, '✕ 关闭按钮应存在').not.toBeNull()
    ;(closeBtn as HTMLElement).click()
    await flushPromises()

    expect(onUpdateShow).toHaveBeenCalledWith(false)
  })
})

describe('AppModal 卡牌弹窗视觉规范（issue #631 cardSize 分档与默认无边框）', () => {
  it.each([
    ['sm', '420px'],
    ['md', '480px'],
    ['lg', '560px'],
  ])('cardSize=%s 等价产出宽度 %s', async (cardSize, width) => {
    mountModal({ cardSize })
    await flushPromises()

    expect(findCard().style.width).toBe(width)
  })

  it('未传 cardSize 时不注入宽度：既有调用点的 style 宽度原样生效', async () => {
    mountModal({ style: 'width: 440px' })
    await flushPromises()

    expect(findCard().style.width).toBe('440px')
  })

  it('cardSize 与调用方显式 style 并存时显式 style 胜出（向后兼容护栏）', async () => {
    mountModal({ cardSize: 'md', style: 'width: 440px' })
    await flushPromises()

    expect(findCard().style.width).toBe('440px')
  })

  it('默认无边框：未传 bordered 的卡片弹窗不渲染边框', async () => {
    mountModal()
    await flushPromises()

    expect(findCard().classList.contains('n-card--bordered')).toBe(false)
  })

  it('显式 bordered=true 逃逸门：边框照常渲染', async () => {
    mountModal({ bordered: true })
    await flushPromises()

    expect(findCard().classList.contains('n-card--bordered')).toBe(true)
  })
})

describe('AppModal 弹窗移动档（issue #844 / ADR-0088 决策 8 全屏化分支）', () => {
  it.each(['sm', 'md', 'lg'] as const)(
    '移动档近全屏卡片：cardSize=%s 三档宽度不生效，改挂移动钩子类与视口计算尺寸',
    async (cardSize) => {
      setFakeMedia({ width: 390 })
      mountModal({ cardSize })
      await flushPromises()

      const card = findCard()
      expect(card.classList.contains(MOBILE_CARD_CLASS)).toBe(true)
      expect(card.style.width).toBe('calc(100vw - 32px)')
      expect(card.style.height).toContain('100dvh')
    },
  )

  it('移动档未声明 cardSize 同样全屏化（移动档以全屏化为唯一形态）', async () => {
    setFakeMedia({ width: 390 })
    mountModal()
    await flushPromises()

    expect(findCard().classList.contains(MOBILE_CARD_CLASS)).toBe(true)
    expect(findCard().style.width).toBe('calc(100vw - 32px)')
  })

  it('桌面档不带移动钩子类，cardSize 宽度照常（sm/md/lg 排版不变）', async () => {
    mountModal({ cardSize: 'md' })
    await flushPromises()

    expect(findCard().classList.contains(MOBILE_CARD_CLASS)).toBe(false)
    expect(findCard().style.width).toBe('480px')
  })

  it('缩窗实时换档：桌面 480px → 移动全屏 → 回桌面恢复 480px', async () => {
    mountModal({ cardSize: 'md' })
    await flushPromises()
    expect(findCard().style.width).toBe('480px')

    setFakeMedia({ width: 390 })
    await nextTick()
    expect(findCard().classList.contains(MOBILE_CARD_CLASS)).toBe(true)
    expect(findCard().style.width).toBe('calc(100vw - 32px)')

    setFakeMedia({ width: 1280 })
    await nextTick()
    expect(findCard().classList.contains(MOBILE_CARD_CLASS)).toBe(false)
    expect(findCard().style.width).toBe('480px')
  })

  it('移动档遮罩点击仍一律不关（弹层关闭语义零变化）', async () => {
    setFakeMedia({ width: 390 })
    const { onUpdateShow } = mountModal()
    await flushPromises()

    await pressReleaseOnMask()
    expect(onUpdateShow).not.toHaveBeenCalled()
  })

  it('移动档标题栏 ✕ 照常关闭', async () => {
    setFakeMedia({ width: 390 })
    const { onUpdateShow } = mountModal({ closable: true })
    await flushPromises()

    const closeBtn = document.body.querySelector('.n-base-close')
    expect(closeBtn, '✕ 关闭按钮应存在').not.toBeNull()
    ;(closeBtn as HTMLElement).click()
    await flushPromises()

    expect(onUpdateShow).toHaveBeenCalledWith(false)
  })

  it('移动档 ESC 照常关闭（弹层库默认行为，关闭通道零变化）', async () => {
    setFakeMedia({ width: 390 })
    const { onUpdateShow } = mountModal()
    await flushPromises()

    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'Escape', bubbles: true }))
    await flushPromises()

    expect(onUpdateShow).toHaveBeenCalledWith(false)
  })

  it('移动档开/关上报弹层注册表照常（快捷键抑制不回归）', async () => {
    setFakeMedia({ width: 390 })
    resetOverlays()
    // 受控宿主：以 show 状态驱动 AppModal（生产用法 :show 绑定的等价形态）
    const wrapper = mount(
      defineComponent({
        components: { AppModal },
        setup() {
          const show = ref(true)
          return { show }
        },
        template: `
          <AppModal :show="show" title="契约弹窗" preset="card">
            <div class="app-modal-marker">内容</div>
          </AppModal>
        `,
      }),
    )
    await flushPromises()

    expect(hasOpenOverlay()).toBe(true)
    expect(openOverlayNames()).toContain('modal')

    ;(wrapper.vm as unknown as { show: boolean }).show = false
    await flushPromises()
    expect(hasOpenOverlay()).toBe(false)
  })
})

describe('代表性弹窗移动档挂载（issue #844 验收：调用点零改动自动获得全屏化）', () => {
  it('确认框族（AppDangerConfirmModal）移动档呈全屏化结构', async () => {
    setFakeMedia({ width: 390 })
    mount(AppDangerConfirmModal, {
      props: {
        level: 'warning',
        show: true,
        title: '危险确认',
        confirmText: '确认',
        cancelText: '取消',
        onConfirm: () => {},
        onCancel: () => {},
      },
    })
    await flushPromises()

    const card = findCard()
    expect(card.classList.contains(MOBILE_CARD_CLASS)).toBe(true)
    expect(card.style.width).toBe('calc(100vw - 32px)')
    // 确认框内容（说明段 + 按钮行）照常渲染于卡片内
    expect(document.body.querySelector('[data-testid="danger-confirm"]')).not.toBeNull()
  })
})
