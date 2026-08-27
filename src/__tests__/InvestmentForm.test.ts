import { describe, it, expect, vi, beforeEach, beforeAll } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { NSelect } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import InvestmentForm from '@/components/InvestmentForm.vue'
import type { Account, Currency, Instrument } from '@/types'

const mockInvoke = vi.mocked(invoke)

// jsdom 不实现 scrollTo：naive-ui 打开虚拟滚动下拉时会调用，提前 polyfill 避免 unhandled rejection
beforeAll(() => {
  Element.prototype.scrollTo = () => {}
})

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '证券户',
    type: 'investment',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

const mockInstruments: Instrument[] = [
  {
    id: 'ins-1',
    symbol: 'NVDA',
    name: '英伟达',
    type: 'stock',
    currency_code: 'CNY',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
  },
]

/** 标的下拉 = 带 remote 搜索的那个 NSelect */
function instrumentSelect(wrapper: ReturnType<typeof mount>) {
  return wrapper.findAllComponents(NSelect).find((s) => s.props('remote'))!
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_instruments') return Promise.resolve({ items: [], total: 0 })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('InvestmentForm.vue 移除「新增标的」入口（issue #152）', () => {
  it('不渲染「新增标的」切换按钮与内嵌建档表单', () => {
    const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
    // 无「新增标的」按钮
    const newBtn = wrapper.findAll('button').find((b) => b.text().includes('新增标的'))
    expect(newBtn).toBeUndefined()
    // 无内嵌建档表单的字样
    expect(wrapper.text()).not.toContain('保存标的')
    expect(wrapper.text()).not.toContain('新增标的')
  })

  it('标的无候选时下拉空态提示「未找到标的，可通过同步或 AI 导入新增」', async () => {
    const wrapper = mount(InvestmentForm, {
      props: { kind: 'buy', submitLabel: '记买入' },
      attachTo: document.body,
    })
    // 用户点开标的选择（此时无候选）→ 下拉菜单渲染空态文案
    await instrumentSelect(wrapper).find('.n-base-selection').trigger('click')
    await flushPromises()
    expect(document.body.textContent).toContain('未找到标的，可通过同步或 AI 导入新增')
  })

  it('标的搜索与选择行为不受影响：搜索触发 list_instruments、候选可选择', async () => {
    vi.useFakeTimers()
    try {
      const wrapper = mount(InvestmentForm, { props: { kind: 'buy', submitLabel: '记买入' } })
      const select = instrumentSelect(wrapper)
      // 用户在标的选择框输入 → 触发远程搜索（防抖 300ms）
      await select.find('input').setValue('NVDA')
      await vi.advanceTimersByTimeAsync(300)
      const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
      expect(calls).toHaveLength(1)
      const [, args] = calls[0] as [string, { filter: { search: string } }]
      expect(args.filter.search).toBe('NVDA')
      // 返回候选后可选择
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'list_instruments') return Promise.resolve({ items: mockInstruments, total: 1 })
        return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
      })
      await select.find('input').setValue('NVDA')
      await vi.advanceTimersByTimeAsync(300)
      await flushPromises()
      expect((select.props('options') as { value: string }[]).map((o) => o.value)).toEqual(['ins-1'])
      select.vm.$emit('update:value', 'ins-1')
      await flushPromises()
      expect(select.props('value')).toBe('ins-1')
    } finally {
      vi.useRealTimers()
    }
  })
})
