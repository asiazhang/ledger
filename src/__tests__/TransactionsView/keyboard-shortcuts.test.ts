import { mountView } from './common'
import { describe, it, expect, beforeEach } from 'vitest'
import { flushPromises } from '@vue/test-utils'
import { NModal } from 'naive-ui'
import TransactionForm from '@/components/TransactionForm.vue'

describe('TransactionsView 裸键快捷键（issue #153）', () => {
  // jsdom 的 document.body 跨测试共享：清掉前序测试遗留的弹层容器，避免误抑制
  beforeEach(() => {
    document.body.innerHTML = ''
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

  it('弹层打开时按键不触发；弹窗打开后再按键不换类型', async () => {
    const wrapper = await mountView()
    // 先造一个弹层（与真实弹窗打开时同样会出现的遮罩元素）
    const overlay = document.createElement('div')
    overlay.className = 'n-modal-mask'
    document.body.appendChild(overlay)
    pressKey('a')
    await flushPromises()
    expect(wrapper.findComponent(NModal).props('show')).toBe(false)
    overlay.remove()
    // 真实打开支出弹窗后再按 z：抑制触发，不切换为转账
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
