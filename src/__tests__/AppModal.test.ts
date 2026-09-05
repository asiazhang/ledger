import { describe, it, expect, vi, afterEach } from 'vitest'
import { mount, enableAutoUnmount, flushPromises } from '@vue/test-utils'
import AppModal from '@/components/AppModal.vue'

// NModal 内容传送至 document.body：每测后卸载，避免弹窗残留污染下一用例查询
enableAutoUnmount(afterEach)

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
