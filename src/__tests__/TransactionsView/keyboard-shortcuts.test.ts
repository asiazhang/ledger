import { mountView } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NModal } from 'naive-ui'
import { createOverlayToken, resetOverlays } from '@/composables/overlayRegistry'
import TransactionForm from '@/components/TransactionForm.vue'

describe('TransactionsView 裸键快捷键（issue #153）', () => {
  // jsdom 的 document.body 跨测试共享；注册表是模块级状态，同样需要复位
  beforeEach(() => {
    document.body.innerHTML = ''
    resetOverlays()
  })

  function pressKey(key: string) {
    window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
  }

  it.each([
    ['a', '支出', 'expense'],
    ['z', '转账', 'transfer'],
    ['i', '收入', 'income'],
    ['b', '买入', 'buy'],
    ['s', '卖出', 'sell'],
  ] as const)('裸键 %s 直达「记一笔 · %s」弹窗（与下拉同一入口）', async (key, kindLabel, kind) => {
    const wrapper = await mountView()
    pressKey(key)
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(true)
    expect(wrapper.findComponent(NModal).props('title')).toBe(`记一笔 · ${kindLabel}`)
    expect(wrapper.findComponent(TransactionForm).props('kind')).toBe(kind)
  })

  it('焦点在输入框时按键不触发', async () => {
    const wrapper = await mountView()
    const input = document.createElement('input')
    document.body.appendChild(input)
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', bubbles: true }))
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
  })

  it('弹层上报打开时按键不触发；弹窗打开后再按键不换类型', async () => {
    const wrapper = await mountView()
    // 先造一个打开中的弹层信号（真实场景由 AppModal/AppSelect 等封装上报）
    const token = createOverlayToken('modal')
    token.set(true)
    pressKey('a')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    token.set(false)
    // 真实打开支出弹窗后再按 z：AppModal 已上报打开状态，抑制触发，不切换为转账
    pressKey('a')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 支出')
    pressKey('z')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('title')).toBe('记一笔 · 支出')
    expect(wrapper.findComponent(TransactionForm).props('kind')).toBe('expense')
  })

  it('非映射键与带修饰键不触发', async () => {
    const wrapper = await mountView()
    pressKey('x')
    window.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'a', bubbles: true, metaKey: true }),
    )
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
  })
})
