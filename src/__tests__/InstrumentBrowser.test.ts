import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises, enableAutoUnmount } from '@vue/test-utils'
import { h, nextTick } from 'vue'
import { NDialogProvider } from 'naive-ui'
import { setActivePinia, createPinia } from 'pinia'
import { listen } from '@tauri-apps/api/event'
import { useReferenceStore } from '@/stores/reference'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import { makeInstrument } from './factories'
import {
  firePricesChanged,
  resetPricesChangedHandler,
} from './prices-changed-mock'
import { stubReferenceInvoke } from './helpers/reference-stubs'
import type { Instrument, SyncProgress } from '@/types'

const mockListen = vi.mocked(listen)

// 价格失效信号订阅基座 mock（issue #238 / ADR-0031 决策 3）：捕获订阅回调，
// 测试中手动触发模拟后端 emit；捕获/触发辅助收在 prices-changed-mock 共享。
vi.mock('@/composables/usePricesChanged', async () => {
  const { capturePricesChangedHandler } = await import('./prices-changed-mock')
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  }
})

// NModal 内容 teleport 到 document.body，须在每个测试后卸载 wrapper 并清空 body，
// 否则上一个测试遗留的弹窗 DOM 会污染下一个测试（bodyQuery 拿到陈旧元素）。
enableAutoUnmount(afterEach)
afterEach(() => {
  document.body.innerHTML = ''
})

/** 组件顶层调用 useAppDialog（删除二次确认，issue #292），与 App.vue 同构需
 * NDialogProvider 包裹（先例：AccountsView.test.ts 的 mountView）。 */
function mountBrowser() {
  return mount(NDialogProvider, {
    slots: { default: () => h(InstrumentBrowser) },
  })
}

// 捕获全量同步进度事件回调，便于在组件测试中模拟 sync-instruments:progress
let syncProgressHandler: ((event: { payload: SyncProgress }) => void) | undefined


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
    source: 'eastmoney',
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
    source: 'eastmoney',
    price_cents: 1200,
    invested: false,
  },
]

function baseInvoke(
  extra?: Record<string, (args?: Record<string, unknown>) => unknown>,
) {
  stubReferenceInvoke({
    list_accounts: [],
    list_categories: [],
    list_insurers: [],
    list_merchants: [],
    list_instruments: { items: mockInstruments, total: mockInstruments.length },
    sync_instruments: () => Promise.resolve(undefined),
    cancel_sync_instruments: () =>
      Promise.resolve({ cancelled: false, message: '当前没有正在进行的同步' }),
    ...extra,
  })
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockListen.mockReset()
  syncProgressHandler = undefined
  resetPricesChangedHandler()
  mockListen.mockImplementation((_event, handler) => {
    syncProgressHandler = handler as (event: { payload: SyncProgress }) => void
    return Promise.resolve(() => {})
  })
  baseInvoke()
  localStorage.clear()
  const store = useReferenceStore()
  await store.refresh()
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
    const wrapper = mountBrowser()
    await flushPromises()
    expect(wrapper.find('[data-testid="sync-holding-prices"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('同步持仓价格')
  })

  it('工具栏包含「只看持仓」开关', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    expect(wrapper.find('[data-testid="only-invested-switch"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('只看持仓')
  })

  it('勾选「只看持仓」后标的查询携带 only_invested=true', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    const sw = wrapper.find('[data-testid="only-invested-switch"]')
    await sw.trigger('click')
    await flushPromises()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    const [, args] = calls[calls.length - 1]
    expect(args.filter).toMatchObject({ only_invested: true })
  })

  it('未勾选「只看持仓」时标的查询 only_invested 为 null', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    const [, args] = calls[calls.length - 1]
    expect(args.filter).toMatchObject({ only_invested: null })
  })
})

describe('InstrumentBrowser 持仓标记列', () => {
  it('持仓标的显示「持仓」标记，未持仓显示 -', async () => {
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('无持仓标的可同步')
  })

  it('同步失败显示错误消息', async () => {
    baseInvoke({
      sync_holding_prices: () => Promise.reject(new Error('网络错误')),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="sync-holding-prices"]').trigger('click')
    await flushPromises()
    // 失败消息应包含具体原因，而非字符串化的 [object Object]
    expect(wrapper.text()).toContain('同步失败：网络错误')
  })
})

describe('InstrumentBrowser 价格失效信号（issue #238 / ADR-0031）', () => {
  it('信号触发后恰好重拉一次当前页查询', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    firePricesChanged()
    await flushPromises()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    expect(calls.length).toBe(before + 1)
    // 查询参数与初始加载同形：原地刷新，不改变搜索/筛选状态
    const [, args] = calls.at(-1)!
    expect(args).toMatchObject({
      filter: { search: null, market: null, only_invested: null, page: 1 },
    })
  })

  it('信号触发后原地重拉，保留分页状态（不重置到第 1 页抽走视线下的行）', async () => {
    // 60 只标的 → 两页（pageSize 50），按页切片返回不同内容
    const pool = Array.from({ length: 60 }, (_, i) =>
      makeInstrument({ id: `inst-${i + 1}`, symbol: String(600000 + i) }),
    )
    baseInvoke({
      list_instruments: (args?: Record<string, unknown>) => {
        const page = (args?.filter as { page?: number } | undefined)?.page ?? 1
        return Promise.resolve({
          items: pool.slice((page - 1) * 50, page * 50),
          total: pool.length,
        })
      },
    })
    const wrapper = mountBrowser()
    await flushPromises()
    const pageSymbols = () =>
      wrapper.findAll('td[data-col-key="symbol"]').map((c) => c.text())
    expect(pageSymbols()).toEqual(pool.slice(0, 50).map((i) => i.symbol))

    // 翻到第 2 页（分页项为可点击 div，无内层 button）
    const page2 = wrapper.findAll('.n-pagination-item').find((el) => el.text() === '2')
    expect(page2).toBeTruthy()
    await page2!.trigger('click')
    await flushPromises()
    expect(pageSymbols()).toEqual(pool.slice(50).map((i) => i.symbol))

    // 信号触发：原地重拉第 2 页，而非 reload() 重置回第 1 页
    firePricesChanged()
    await flushPromises()
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments')
    const [, args] = calls.at(-1)!
    expect((args as { filter: { page: number } }).filter.page).toBe(2)
    expect(pageSymbols()).toEqual(pool.slice(50).map((i) => i.symbol))
  })
})

describe('InstrumentBrowser 全量同步（issue #109）', () => {
  it('工具栏包含「全量同步」按钮', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    const btn = wrapper.find('[data-testid="full-sync"]')
    expect(btn.exists()).toBe(true)
    expect(wrapper.text()).toContain('全量同步')
  })

  it('点击全量同步先弹确认框，未确认不调用 sync_instruments', async () => {
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
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
    const wrapper = mountBrowser()
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

describe('InstrumentBrowser 添加基金（issue #301 / ADR-0038）', () => {
  const fundResult = {
    instrument_id: 'inst-fund-1',
    symbol: '000001',
    name: '华夏成长混合',
    fund_class: '混合型-灵活',
    nav_cents: 13180,
    nav_date: '2026-08-28',
    price_written: true,
  }

  async function openAddFundModal() {
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="add-fund"]').trigger('click')
    await nextTick()
    return wrapper
  }

  async function setCode(code: string) {
    // data-testid 落在 NInput 根元素上，真正受控的是内部 input 元素：
    // 原生赋值 + 冒泡 input 事件驱动 v-model 更新。
    const input = bodyQuery('[data-testid="add-fund-code"]')!.querySelector('input')!
    input.value = code
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await nextTick()
    await flushPromises()
    await nextTick()
  }

  it('工具栏包含「添加基金」按钮', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    const btn = wrapper.find('[data-testid="add-fund"]')
    expect(btn.exists()).toBe(true)
    expect(btn.text()).toContain('添加基金')
  })

  it('点击打开弹窗；非 6 位数字时提交按钮禁用、不发请求', async () => {
    baseInvoke({ add_fund_by_code: () => Promise.resolve(fundResult) })
    await openAddFundModal()
    expect(bodyQuery('[data-testid="add-fund-code"]')).not.toBeNull()
    // 空码 / 位数不足：提交禁用
    expect(
      (bodyQuery('[data-testid="submit-add-fund"]') as HTMLButtonElement).disabled,
    ).toBe(true)
    await setCode('1234')
    expect(
      (bodyQuery('[data-testid="submit-add-fund"]') as HTMLButtonElement).disabled,
    ).toBe(true)
    expect(mockInvoke).not.toHaveBeenCalledWith('add_fund_by_code', { code: '1234' })
  })

  it('输入过滤非数字字符（粘贴字母只剩数字）', async () => {
    await openAddFundModal()
    await setCode('12a3b4')
    // watch 过滤后应为 1234（未满 6 位仍禁用），输满 6 位数字后可用
    expect(
      (bodyQuery('[data-testid="submit-add-fund"]') as HTMLButtonElement).disabled,
    ).toBe(true)
    await setCode('000001')
    expect(
      (bodyQuery('[data-testid="submit-add-fund"]') as HTMLButtonElement).disabled,
    ).toBe(false)
  })

  // 弹窗退场过渡（NModal ~200ms）后断言 DOM 已卸载
  async function waitForModalLeave() {
    await new Promise((r) => setTimeout(r, 300))
    await nextTick()
    await flushPromises()
  }

  it('有效代码提交：调用 add_fund_by_code，成功回执展示名称/分类/净值/日期并重拉列表', async () => {
    baseInvoke({ add_fund_by_code: () => Promise.resolve(fundResult) })
    const wrapper = await openAddFundModal()
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    await setCode('000001')
    await clickBody('[data-testid="submit-add-fund"]')
    expect(mockInvoke).toHaveBeenCalledWith('add_fund_by_code', { code: '000001' })
    await waitForModalLeave()
    // 成功回执（页面级）：名称、代码、分类、4 位小数净值与净值日期
    const msg = wrapper.find('[data-testid="add-fund-result"]')
    expect(msg.exists()).toBe(true)
    expect(msg.text()).toContain('已添加基金：华夏成长混合（000001 · 混合型-灵活）')
    expect(msg.text()).toContain('最新净值 1.318（2026-08-28）')
    // 列表重拉（新标的上列表）；弹窗关闭的 DOM 断言受 NModal 退场过渡影响
    //（jsdom 不触发 transitionend，先例：全量同步确认框测试交状态层覆盖），此处不断言。
    const after = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    expect(after).toBeGreaterThan(before)
  })

  it('未取到净值：仍添加成功，回执提示暂未取到净值', async () => {
    baseInvoke({
      add_fund_by_code: () =>
        Promise.resolve({
          ...fundResult,
          nav_cents: null,
          nav_date: null,
          price_written: false,
        }),
    })
    const wrapper = await openAddFundModal()
    await setCode('012345')
    await clickBody('[data-testid="submit-add-fund"]')
    const msg = wrapper.find('[data-testid="add-fund-result"]')
    expect(msg.text()).toContain('暂未取到净值')
  })

  it('查无此码：弹窗内展示中文报错，不重拉列表、无成功回执', async () => {
    baseInvoke({
      add_fund_by_code: () =>
        Promise.reject({ kind: 'Invalid', message: '查无基金代码 999999，请核对后重试' }),
    })
    const wrapper = await openAddFundModal()
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    await setCode('999999')
    await clickBody('[data-testid="submit-add-fund"]')
    // 中文报错在弹窗内展示（AppError 序列化对象形态经 errorMessage 提取），
    // 弹窗保持打开供改码重试（DOM 卸载断言受退场过渡影响，以错误区呈现为准）
    const err = bodyQuery('[data-testid="add-fund-error"]')!
    expect(err).not.toBeNull()
    expect(err.textContent).toContain('查无基金代码 999999')
    expect(err.textContent).not.toContain('[object Object]')
    // 失败不产生标的行：无成功回执、不重拉列表
    expect(wrapper.find('[data-testid="add-fund-result"]').exists()).toBe(false)
    const after = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    expect(after).toBe(before)
  })
})

describe('InstrumentBrowser 来源列与新建标的入口（issue #290 / ADR-0036）', () => {
  it('来源列渲染：同步标的显示「同步」，手动标的显示「手动」标记', async () => {
    baseInvoke({
      list_instruments: () =>
        Promise.resolve({
          items: [
            ...mockInstruments,
            makeInstrument({ id: 'inst-3', symbol: '稳稳地幸福', type: 'other', name: '稳稳地幸福', market: 'unknown', source: 'manual' }),
          ],
          total: 3,
        }),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    const sourceCells = wrapper.findAll('td[data-col-key="source"]')
    expect(sourceCells.map((c) => c.text())).toEqual(['同步', '同步', '手动'])
    // 手动标的带突出标记（tag），同步标的为纯文本
    expect(wrapper.findAll('[data-testid="source-manual"]').length).toBe(1)
  })

  it('「新建标的」按钮打开创建弹窗', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="create-instrument"]').trigger('click')
    await nextTick()
    expect(bodyQuery('[data-testid="create-instrument-name"]')).not.toBeNull()
    expect(document.body.textContent).toContain('新建标的')
  })

  it('创建成功：页面级回执 + 列表重拉（回到第 1 页）', async () => {
    baseInvoke({
      create_instrument: () => Promise.resolve('inst-new'),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="create-instrument"]').trigger('click')
    await nextTick()
    // 经组件 emit 驱动（弹窗内表单校验与提交流程由 CreateInstrumentModal.test.ts 覆盖）
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    wrapper.findComponent({ name: 'CreateInstrumentModal' }).vm.$emit(
      'created',
      '已创建标的：稳稳地幸福（稳稳地幸福）',
    )
    await flushPromises()
    const msg = wrapper.find('[data-testid="create-instrument-result"]')
    expect(msg.exists()).toBe(true)
    expect(msg.text()).toContain('已创建标的：稳稳地幸福（稳稳地幸福）')
    const after = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    expect(after).toBe(before + 1)
  })
})

describe('InstrumentBrowser 自建标的删除（issue #292 / ADR-0036）', () => {
  function manualRow() {
    return makeInstrument({
      id: 'inst-manual',
      symbol: '稳稳地幸福',
      type: 'other',
      name: '稳稳地幸福',
      market: 'unknown',
      source: 'manual',
      invested: false,
    })
  }

  function listWith(...items: Instrument[]) {
    baseInvoke({
      list_instruments: () => Promise.resolve({ items, total: items.length }),
    })
  }

  /** NDialog 渲染到 document.body，按文本定位确认/取消按钮并原生触发点击。 */
  async function clickDialogButton(text: string) {
    const btn = Array.from(document.body.querySelectorAll('.n-dialog button')).find(
      (b) => b.textContent?.trim() === text,
    )
    if (!btn) throw new Error(`dialog 中未找到按钮: ${text}`)
    btn.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await nextTick()
    await flushPromises()
    await new Promise((r) => setTimeout(r, 30))
  }

  it('删除按钮仅手动标的行渲染，同步行无删除动作', async () => {
    listWith(mockInstruments[0]!, manualRow())
    const wrapper = mountBrowser()
    await flushPromises()
    expect(wrapper.find('[data-testid="delete-instrument-稳稳地幸福"]').exists()).toBe(true)
    expect(wrapper.find('[data-testid="delete-instrument-600000"]').exists()).toBe(false)
  })

  it('点击删除弹确认框（含标的名称）；取消不调用 delete_instrument', async () => {
    listWith(manualRow())
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="delete-instrument-稳稳地幸福"]').trigger('click')
    await nextTick()
    // 确认框出现，含标的名称
    expect(document.body.querySelector('.n-dialog')).not.toBeNull()
    expect(document.body.querySelector('.n-dialog')!.textContent).toContain('稳稳地幸福')
    // 未确认：不调用删除命令
    expect(mockInvoke).not.toHaveBeenCalledWith('delete_instrument', { id: 'inst-manual' })
    await clickDialogButton('取消')
    expect(mockInvoke).not.toHaveBeenCalledWith('delete_instrument', { id: 'inst-manual' })
  })

  it('确认后调用 delete_instrument，列表原地重拉并显示成功回执', async () => {
    baseInvoke({
      list_instruments: () => Promise.resolve({ items: [manualRow()], total: 1 }),
      delete_instrument: () => Promise.resolve(),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    await wrapper.find('[data-testid="delete-instrument-稳稳地幸福"]').trigger('click')
    await nextTick()
    await clickDialogButton('删除')
    expect(mockInvoke).toHaveBeenCalledWith('delete_instrument', { id: 'inst-manual' })
    const msg = wrapper.find('[data-testid="delete-instrument-result"]')
    expect(msg.exists()).toBe(true)
    expect(msg.text()).toContain('已删除标的：稳稳地幸福')
    const after = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    expect(after).toBe(before + 1)
  })

  it('删除失败（如已产生买卖流水）：显示后端中文错误，不重拉列表', async () => {
    baseInvoke({
      list_instruments: () => Promise.resolve({ items: [manualRow()], total: 1 }),
      delete_instrument: () =>
        Promise.reject({
          kind: 'Invalid',
          message: '该标的已有买卖流水，无法删除：可先删除相关交易后再试',
        }),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    await wrapper.find('[data-testid="delete-instrument-稳稳地幸福"]').trigger('click')
    await nextTick()
    await clickDialogButton('删除')
    const msg = wrapper.find('[data-testid="delete-instrument-result"]')
    expect(msg.exists()).toBe(true)
    expect(msg.text()).toContain('已有买卖流水')
    expect(msg.text()).not.toContain('[object Object]')
    // 失败不重拉列表
    const after = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    expect(after).toBe(before)
  })
})

describe('InstrumentBrowser 行内录价入口（issue #291 / ADR-0036）', () => {
  /** 五类行覆盖录价分区：股票/真实代码基金无入口；自建标的与名称充代码基金行有入口 */
  function rowsForQuoteGating() {
    return [
      makeInstrument({ id: 'inst-st', symbol: '600000', type: 'stock', source: 'eastmoney' }),
      makeInstrument({ id: 'inst-fund6', symbol: '000001', type: 'fund', source: 'manual', market: 'unknown' }),
      makeInstrument({ id: 'inst-other', symbol: '稳稳地幸福', type: 'other', source: 'manual', market: 'unknown' }),
      makeInstrument({ id: 'inst-bond', symbol: '019547', type: 'bond', source: 'manual', market: 'unknown' }),
      makeInstrument({ id: 'inst-fund-name', symbol: '稳稳地幸福', type: 'fund', source: 'manual', market: 'unknown' }),
    ]
  }

  it('录价入口只对同步覆盖不到的标的开放：股票与 6 位代码基金无入口，自建标的与名称充代码基金行有入口', async () => {
    const rows = rowsForQuoteGating()
    baseInvoke({
      list_instruments: () => Promise.resolve({ items: rows, total: rows.length }),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    const cells = wrapper.findAll('td[data-col-key="quote"]').map((c) => c.text())
    expect(cells).toEqual(['-', '-', '录价', '录价', '录价'])
    // 入口按钮带标的定位 testid（有入口的三行）
    expect(wrapper.find('[data-testid="quote-600000"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="quote-000001"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="quote-稳稳地幸福"]').exists()).toBe(true)
  })

  it('点击行内「录价」打开报价弹窗，弹窗内展示标的代码', async () => {
    const rows = [rowsForQuoteGating()[2]]
    baseInvoke({
      list_instruments: () => Promise.resolve({ items: rows, total: rows.length }),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="quote-稳稳地幸福"]').trigger('click')
    await nextTick()
    expect(document.body.textContent).toContain('录价 — 稳稳地幸福')
  })

  it('录价成功：页面级回执 + 列表零手动重拉（刷新由价格失效信号驱动）', async () => {
    baseInvoke({
      record_manual_price: () =>
        Promise.resolve({ history_written: true, current_price_written: true }),
    })
    const wrapper = mountBrowser()
    await flushPromises()
    // 弹窗内校验与提交流由 ManualPriceModal.test.ts 覆盖，此处经组件 emit 驱动
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    wrapper.findComponent({ name: 'ManualPriceModal' }).vm.$emit(
      'quoted',
      '已录价：稳稳地幸福 现价更新为 1.318',
    )
    await flushPromises()
    // 页面级回执展示
    const msg = wrapper.find('[data-testid="manual-quote-result"]')
    expect(msg.exists()).toBe(true)
    expect(msg.text()).toContain('已录价：稳稳地幸福 现价更新为 1.318')
    // 调用方零手动重拉：录价回执不触发列表查询
    const after = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    expect(after).toBe(before)
    // 列表刷新由价格失效信号驱动（后端实际写入后广播）：信号触发后恰好重拉一次
    firePricesChanged()
    await flushPromises()
    const refreshed = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    expect(refreshed).toBe(before + 1)
  })
})
