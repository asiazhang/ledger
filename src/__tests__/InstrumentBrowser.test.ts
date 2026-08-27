import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import type { Currency, Instrument } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

const mockInstruments: Instrument[] = [
  {
    id: 'inst-1',
    symbol: '600000',
    type: 'stock',
    name: '浦发银行',
    currency_code: 'CNY',
    market: 'sh',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    price_cents: 1000,
    invested: true,
  },
  {
    id: 'inst-2',
    symbol: '000001',
    type: 'stock',
    name: '平安银行',
    currency_code: 'CNY',
    market: 'sz',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    price_cents: 1200,
    invested: false,
  },
]

function baseInvoke(
  extra?: Record<string, (cmd: string) => unknown>,
) {
  mockInvoke.mockImplementation((cmd: string) => {
    if (extra?.[cmd]) return extra[cmd](cmd)
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve([])
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_instruments')
      return Promise.resolve({ items: mockInstruments, total: mockInstruments.length })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

describe('InstrumentBrowser 标的页工具栏', () => {
  it('工具栏包含「同步持仓价格」按钮', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    expect(wrapper.find('[data-testid="sync-holding-prices"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('同步持仓价格')
  })

  it('工具栏包含「只看持仓」开关', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    expect(wrapper.find('[data-testid="only-invested-switch"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('只看持仓')
  })

  it('勾选「只看持仓」后标的查询携带 only_invested=true', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    const sw = wrapper.find('[data-testid="only-invested-switch"]')
    await sw.trigger('click')
    await flushPromises()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    const [, args] = calls[calls.length - 1]
    expect(args.filter).toMatchObject({ only_invested: true })
  })

  it('未勾选「只看持仓」时标的查询 only_invested 为 null', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    const [, args] = calls[calls.length - 1]
    expect(args.filter).toMatchObject({ only_invested: null })
  })
})

describe('InstrumentBrowser 持仓标记列', () => {
  it('持仓标的显示「持仓」标记，未持仓显示 -', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    // 持仓标记列：持仓标的渲染「持仓」tag，未持仓标的该单元格为「-」
    const investedCells = wrapper.findAll('td[data-col-key="invested"]')
    expect(investedCells.length).toBe(2)
    const texts = investedCells.map((c) => c.text())
    expect(texts).toContain('持仓')
    expect(texts).toContain('-')
  })
})

describe('InstrumentBrowser 同步持仓价格按钮', () => {
  it('点击按钮触发 sync_holding_prices，进行中按钮 loading', async () => {
    let resolveSync!: (v: unknown) => void
    baseInvoke({
      sync_holding_prices: () =>
        new Promise((res) => {
          resolveSync = res
        }),
    })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    const btn = wrapper.find('[data-testid="sync-holding-prices"]')
    await btn.trigger('click')
    await nextTick()
    expect(resolveSync).toBeDefined()
    expect(wrapper.find('.n-button--loading').exists()).toBe(true)
    resolveSync({ synced: 2, skipped: 1, message: '已同步 2 只，跳过 1 只' })
    await flushPromises()
    expect(wrapper.find('.n-button--loading').exists()).toBe(false)
  })

  it('同步成功显示结果消息', async () => {
    baseInvoke({
      sync_holding_prices: () =>
        Promise.resolve({ synced: 2, skipped: 1, message: '已同步 2 只，跳过 1 只' }),
    })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已同步 2 只，跳过 1 只')
  })

  it('无持仓时同步不报错并提示「无持仓标的可同步」', async () => {
    baseInvoke({
      list_instruments: () => Promise.resolve({ items: [], total: 0 }),
      sync_holding_prices: () =>
        Promise.resolve({ synced: 0, skipped: 0, message: '无持仓标的可同步' }),
    })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('无持仓标的可同步')
  })

  it('同步失败显示错误消息', async () => {
    baseInvoke({
      sync_holding_prices: () => Promise.reject(new Error('网络错误')),
    })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await flushPromises()
    // 失败消息应包含具体原因，而非字符串化的 [object Object]
    expect(wrapper.text()).toContain('同步失败：网络错误')
  })
})
