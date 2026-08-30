import { describe, it, expect, vi, afterEach } from 'vitest'
import { defineComponent, h } from 'vue'
import { mount } from '@vue/test-utils'
import {
  CREATE_KIND_KEYS,
  matchCreateShortcut,
  isEditableTarget,
  useCreateShortcuts,
} from '@/composables/useCreateShortcuts'
import { createOverlayToken, resetOverlays } from '@/composables/overlayRegistry'
import { CREATE_KINDS } from '@/types'
import type { CreateTransactionKind } from '@/types'

function press(
  key: string,
  mods: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean; shiftKey?: boolean } = {},
): KeyboardEvent {
  return new KeyboardEvent('keydown', { key, bubbles: true, ...mods })
}

/** 在指定元素上触发 keydown（event.target 即该元素），返回该事件供 isEditableTarget 断言。 */
function pressOn(target: Element, key = 'a'): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key, bubbles: true })
  target.dispatchEvent(e)
  return e
}

describe('CREATE_KIND_KEYS 键位映射', () => {
  it('恰好覆盖 5 种可创建类型，refund 不占键位', () => {
    expect(Object.keys(CREATE_KIND_KEYS).sort()).toEqual([...CREATE_KINDS].sort())
    expect('refund' in CREATE_KIND_KEYS).toBe(false)
  })

  it('键位分配：a=支出 z=转账 i=收入 b=买入 s=卖出', () => {
    expect(CREATE_KIND_KEYS).toEqual({
      expense: 'a',
      transfer: 'z',
      income: 'i',
      buy: 'b',
      sell: 's',
    })
  })
})

describe('matchCreateShortcut 真值表', () => {
  it.each([
    ['a', 'expense'],
    ['z', 'transfer'],
    ['i', 'income'],
    ['b', 'buy'],
    ['s', 'sell'],
  ] as const)('裸键 %s → %s', (key, kind) => {
    expect(matchCreateShortcut(press(key))).toBe(kind)
  })

  it.each(['ctrlKey', 'metaKey', 'altKey', 'shiftKey'] as const)(
    '带修饰键 %s 的 a 不命中（仅裸键）',
    (mod) => {
      expect(matchCreateShortcut(press('a', { [mod]: true }))).toBeNull()
    },
  )

  it.each(['A', 'r', 'x', '1', 'Escape', 'Enter', ' '])('非映射裸键 %s 不命中', (key) => {
    // 大写 A（如 CapsLock）不命中：精确匹配小写键位
    // r 无键位：退款不占键位
    expect(matchCreateShortcut(press(key))).toBeNull()
  })
})

describe('isEditableTarget 真值表', () => {
  it.each(['input', 'textarea', 'select'])('焦点在 <%s> 上 → true', (tag) => {
    const el = document.createElement(tag)
    document.body.appendChild(el)
    expect(isEditableTarget(pressOn(el))).toBe(true)
    el.remove()
  })

  it('焦点在 contenteditable 元素上 → true', () => {
    const el = document.createElement('div')
    el.setAttribute('contenteditable', '')
    document.body.appendChild(el)
    expect(isEditableTarget(pressOn(el))).toBe(true)
    el.remove()
  })

  it('焦点在可编辑元素的子元素上 → true（冒泡目标为子元素）', () => {
    const wrapper = document.createElement('div')
    const input = document.createElement('input')
    wrapper.appendChild(input)
    document.body.appendChild(wrapper)
    expect(isEditableTarget(pressOn(wrapper))).toBe(false)
    expect(isEditableTarget(pressOn(input))).toBe(true)
    wrapper.remove()
  })

  it.each(['div', 'button', 'body'])('焦点在 <%s> 上 → false', (tag) => {
    const el = document.createElement(tag)
    document.body.appendChild(el)
    expect(isEditableTarget(pressOn(el))).toBe(false)
    el.remove()
  })

  it('事件目标非元素（window）→ false', () => {
    const e = press('a')
    window.dispatchEvent(e)
    expect(isEditableTarget(e)).toBe(false)
  })
})

function mountHost() {
  const open = vi.fn<(k: CreateTransactionKind) => void>()
  const Host = defineComponent({
    setup() {
      useCreateShortcuts(open)
      return () => h('div')
    },
  })
  const wrapper = mount(Host)
  return { open, wrapper }
}

describe('useCreateShortcuts', () => {
  afterEach(() => resetOverlays())

  it('裸键命中时以对应 kind 回调 open', () => {
    const { open } = mountHost()
    window.dispatchEvent(press('a'))
    expect(open).toHaveBeenCalledWith('expense')
    window.dispatchEvent(press('z'))
    expect(open).toHaveBeenCalledWith('transfer')
  })

  it('未命中键位不回调', () => {
    const { open } = mountHost()
    window.dispatchEvent(press('x'))
    window.dispatchEvent(press('a', { metaKey: true }))
    expect(open).not.toHaveBeenCalled()
  })

  it('焦点在可编辑元素上抑制触发', () => {
    const { open } = mountHost()
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.dispatchEvent(press('a'))
    expect(open).not.toHaveBeenCalled()
    input.remove()
  })

  it.each(['modal', 'popconfirm', 'dropdown', 'select', 'date-picker', 'tree-select', 'dialog'])(
    '弹层（%s）打开时抑制触发',
    (name) => {
      const { open } = mountHost()
      const token = createOverlayToken(name)
      token.set(true)
      window.dispatchEvent(press('a'))
      expect(open).not.toHaveBeenCalled()
      token.set(false)
      window.dispatchEvent(press('a'))
      expect(open).toHaveBeenCalledTimes(1)
    },
  )

  it('卸载后不再监听（仅交易页生效的机制基础）', () => {
    const { open, wrapper } = mountHost()
    wrapper.unmount()
    window.dispatchEvent(press('a'))
    expect(open).not.toHaveBeenCalled()
  })
})
