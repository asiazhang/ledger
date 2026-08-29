import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import type { Currency, Instrument, SyncProgress } from '@/types'

const mockListen = vi.mocked(listen)

// NModal 内容 teleport 到 document.body，须在每个测试后卸载 wrapper 并清空 body，
// 否则上一个测试遗留的弹窗 DOM 会污染下一个测试（bodyQuery 拿到陈旧元素）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

// 捕获全量同步进度事件回调，便于在组件测试中模拟 sync-instruments:progress
let syncProgressHandler: ((event: { payload: SyncProgress }) => void) | undefined

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
    if (cmd === 'list_merchants') return Promise.resolve([])
    if (cmd === 'list_instruments')
      return Promise.resolve({ items: mockInstruments, total: mockInstruments.length })
    if (cmd === 'sync_instruments') return Promise.resolve(undefined)
    if (cmd === 'cancel_sync_instruments')
      return Promise.resolve({ cancelled: false, message: '当前没有正在进行的同步' })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  syncProgressHandler = undefined
  mockListen.mockImplementation((_event, handler) => {
    syncProgressHandler = handler as (event: { payload: SyncProgress }) => void
    return Promise.resolve(() => {})
  })
  baseInvoke()
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

// 模拟全量同步进度事件上报（payload 与后端 SyncProgress 一致）
function emitSyncProgress(p: Partial<SyncProgress>) {
  syncProgressHandler?.({
    payload: {
      current: 0,
      total: 0,
      market: '',
      done: false,
      total_inserted: 0,
      total_updated: 0,
      error: null,
      cancelled: false,
      ...p,
    },
  })
}

// NModal 内容默认 teleport 到 document.body，测试需在 body 中查询/触发（wrapper.find 只能查组件根 DOM）。
function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

async function clickBody(selector: string) {
  const el = bodyQuery(selector)
  if (!el) throw new Error(`body 中未找到: ${selector}`)
  // 用原生事件触发，绕过 Naive UI 对 click 的包装
  el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
  // 等响应式刷新 + 微任务 + 几个宏任务，让 NModal 退场过渡结束、内容真正卸载
  await nextTick()
  await flushPromises()
  await new Promise((r) => setTimeout(r, 30))
}

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

describe('InstrumentBrowser 全量同步（issue #109）', () => {
  it('工具栏包含「全量同步」按钮', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    const btn = wrapper.find('[data-testid="full-sync"]')
    expect(btn.exists()).toBe(true)
    expect(wrapper.text()).toContain('全量同步')
  })

  it('点击全量同步先弹确认框，未确认不调用 sync_instruments', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    await nextTick()
    // 确认框出现（含说明 + 开始同步按钮）
    expect(bodyQuery('[data-testid="confirm-full-sync"]')).not.toBeNull()
    expect(document.body.textContent).toContain('涉及数百次 API')
    // 未确认：不调用同步命令
    expect(mockInvoke).not.toHaveBeenCalledWith('sync_instruments')
  })

  it('点击「取消」不调用 sync_instruments（未确认不发起同步）', async () => {
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    // 确认框出现
    expect(bodyQuery('[data-testid="confirm-full-sync"]')).not.toBeNull()
    // 点击「取消」：未确认，不发起同步（确认框关闭的 DOM 断言受过渡影响，交 composable 层覆盖）
    await clickBody('[data-testid="cancel-confirm-full-sync"]')
    expect(mockInvoke).not.toHaveBeenCalledWith('sync_instruments')
  })

  it('确认后调用 sync_instruments，模态框展示进度详情', async () => {
    baseInvoke({ sync_instruments: () => Promise.resolve(undefined) })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    await clickBody('[data-testid="confirm-full-sync"]')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('sync_instruments')
    // 模态进度出现：进度条 + 已处理/总数 + 累计新增/更新 + 中断按钮
    expect(bodyQuery('[data-testid="cancel-full-sync"]')).not.toBeNull()
    expect(document.body.querySelector('.n-progress')).not.toBeNull()
    // 模拟进度事件：current/total/inserted/updated
    emitSyncProgress({ current: 120, total: 300, total_inserted: 5, total_updated: 7 })
    await nextTick()
    expect(bodyQuery('[data-testid="full-sync-count"]')!.textContent).toContain('120')
    expect(bodyQuery('[data-testid="full-sync-count"]')!.textContent).toContain('300')
    expect(bodyQuery('[data-testid="full-sync-cumulative"]')!.textContent).toContain('新增 5')
    expect(bodyQuery('[data-testid="full-sync-cumulative"]')!.textContent).toContain('更新 7')
  })

  it('同步进行中按钮呈「同步中」态，点击重开进度框而非重复同步', async () => {
    let resolveSync!: (v: unknown) => void
    baseInvoke({ sync_instruments: () => new Promise((res) => { resolveSync = res }) })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    await clickBody('[data-testid="confirm-full-sync"]')
    await flushPromises()
    expect(wrapper.find('[data-testid="full-sync"]').text()).toContain('同步中')
    // 同步进行中再次点击入口：不重复触发同步（防重复），而是重开进度框查看进度
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    const syncCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'sync_instruments')
    expect(syncCalls.length).toBe(1)
    // 进度框仍在/已重开（中断按钮存在）——关闭/重开的切换由 composable 层单独覆盖
    expect(bodyQuery('[data-testid="cancel-full-sync"]')).not.toBeNull()
    resolveSync(undefined)
    await flushPromises()
  })

  it('点「中断同步」立即中断，显示「已中断 + 已同步 N 只」', async () => {
    baseInvoke({
      sync_instruments: () => Promise.resolve(undefined),
      cancel_sync_instruments: () =>
        Promise.resolve({ cancelled: true, message: '已请求中断同步' }),
    })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    await clickBody('[data-testid="confirm-full-sync"]')
    await flushPromises()
    // 同步进行中，点击中断
    await clickBody('[data-testid="cancel-full-sync"]')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('cancel_sync_instruments')
    // 终态回流：中断
    emitSyncProgress({ done: true, cancelled: true, total_inserted: 3, total_updated: 2 })
    await nextTick()
    const resultText = bodyQuery('[data-testid="full-sync-result"]')!.textContent!
    expect(resultText).toContain('已中断')
    expect(resultText).toContain('已同步 3 只')
  })

  it('完成时显示新增/更新统计，失败时显示错误', async () => {
    baseInvoke({ sync_instruments: () => Promise.resolve(undefined) })
    const wrapper = mount(InstrumentBrowser)
    await flushPromises()
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    await clickBody('[data-testid="confirm-full-sync"]')
    await flushPromises()
    // 完成
    emitSyncProgress({ done: true, cancelled: false, total_inserted: 10, total_updated: 4 })
    await nextTick()
    const doneText = bodyQuery('[data-testid="full-sync-result"]')!.textContent!
    expect(doneText).toContain('同步完成')
    expect(doneText).toContain('新增 10 只')
    expect(doneText).toContain('更新 4 只')

    // 再次启动后失败
    await wrapper.find('[data-testid="full-sync"]').trigger('click')
    await nextTick()
    await clickBody('[data-testid="confirm-full-sync"]')
    await flushPromises()
    emitSyncProgress({ done: true, error: '请求被限流' })
    await nextTick()
    const errText = bodyQuery('[data-testid="full-sync-result"]')!.textContent!
    expect(errText).toContain('同步失败')
    expect(errText).toContain('请求被限流')
  })
})
