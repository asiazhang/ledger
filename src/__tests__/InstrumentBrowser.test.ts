import { describe, it, expect, vi, beforeEach } from 'vitest'
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { h, nextTick } from 'vue'
import { NDialogProvider } from 'naive-ui'
import { useReferenceStore } from '@/stores/reference'
import InstrumentBrowser from '@/components/investments/InstrumentBrowser.vue'
import { makeInstrument } from './factories'
import {
  firePricesChanged,
  resetPricesChangedHandler,
} from './prices-changed-mock'
import type { Instrument } from '@/types'

// 价格失效信号订阅基座 mock（issue #238 / ADR-0031 决策 3）：捕获订阅回调，
// 测试中手动触发模拟后端 emit；捕获/触发辅助收在 prices-changed-mock 共享。
vi.mock('@/composables/usePricesChanged', async () => {
  const { capturePricesChangedHandler } = await import('./prices-changed-mock')
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  }
})

/** 组件顶层调用 useAppDialog（删除二次确认，issue #292），与 App.vue 同构需
 * NDialogProvider 包裹（先例：AccountsView.test.ts 的 mountView）。 */
function mountBrowser() {
  return mount(NDialogProvider, {
    slots: { default: () => h(InstrumentBrowser) },
  })
}

// 捕获全量同步进度事件回调的基建已随全量同步退役删除（issue #698）。

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

/** 基础布线：beforeEach 安装；用例中途换桩时以模块级表为底再覆写。 */
const BASE_DEFAULTS = {
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
}

beforeEach(async () => {
  resetPricesChangedHandler()
  wireInvokeSeam({ defaults: BASE_DEFAULTS })
  const store = useReferenceStore()
  await store.refresh()
})

// NModal 内容默认 teleport 到 document.body，测试需在 body 中查询（wrapper.find 只能查组件根 DOM）。
function bodyQuery(selector: string): HTMLElement | null {
  return document.body.querySelector(selector)
}

describe('InstrumentBrowser 标的页工具栏', () => {
  it('工具栏包含「同步标的信息」按钮', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('同步标的信息')
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
    expect(lastInvokeArgs('list_instruments').filter).toMatchObject({ only_invested: true })
  })

  it('未勾选「只看持仓」时标的查询 only_invested 为 null', async () => {
    mountBrowser()
    await flushPromises()
    expect(lastInvokeArgs('list_instruments').filter).toMatchObject({ only_invested: null })
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

describe('InstrumentBrowser 同步标的信息按钮', () => {
  it('点击按钮触发 sync_instrument_info，进行中按钮 loading', async () => {
    let resolveSync!: (v: unknown) => void
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: () =>
          new Promise((res) => {
            resolveSync = res
          }),      },
    })
    const wrapper = mountBrowser()
    await flushPromises()
    const btn = wrapper.find('[data-testid="sync-instrument-info"]')
    await btn.trigger('click')
    await nextTick()
    expect(resolveSync).toBeDefined()
    expect(wrapper.find('.n-button--loading').exists()).toBe(true)
    resolveSync({ synced: 2, skipped: 1, message: '已同步 2 只，跳过 1 只' })
    await flushPromises()
    expect(wrapper.find('.n-button--loading').exists()).toBe(false)
  })

  it('同步成功显示结果消息', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: () =>
          Promise.resolve({ synced: 2, skipped: 1, message: '已同步 2 只，跳过 1 只' }),      },
    })
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('已同步 2 只，跳过 1 只')
  })

  it('空库时同步不报错并提示「暂无标的可同步」', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_instruments: () => Promise.resolve({ items: [], total: 0 }),
        sync_instrument_info: () =>
          Promise.resolve({ synced: 0, skipped: 0, message: '暂无标的可同步' }),      },
    })
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('暂无标的可同步')
  })

  it('同步失败显示错误消息', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: () => Promise.reject(new Error('网络错误')),      },
    })
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger('click')
    await flushPromises()
    // 失败消息应包含具体原因，而非字符串化的 [object Object]
    expect(wrapper.text()).toContain('同步失败：网络错误')
  })
})

describe('InstrumentBrowser 价格失效信号（issue #238 / ADR-0031）', () => {
  it('信号触发后恰好重拉一次当前页查询', async () => {
    mountBrowser()
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
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_instruments: (args?: Record<string, unknown>) => {
          const page = (args?.filter as { page?: number } | undefined)?.page ?? 1
          return Promise.resolve({
            items: pool.slice((page - 1) * 50, page * 50),
            total: pool.length,
          })
        },      },
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

describe('InstrumentBrowser 全量同步退役（issue #698 / ADR-0081 决策 3）', () => {
  it('工具栏不再提供全量同步入口：命令、进度/取消事件与前端组装整体退役', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    expect(wrapper.find('[data-testid="full-sync"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('全量同步')
    // 标的信息同步按钮不受退役影响
    expect(wrapper.find('[data-testid="sync-instrument-info"]').exists()).toBe(true)
  })
})

describe('InstrumentBrowser 添加投资标的入口（issue #697 / spec #690）', () => {
  it('工具栏包含「添加投资标的」按钮，点击打开统一对话框；旧「添加基金」「新建标的」入口已退役', async () => {
    const wrapper = mountBrowser()
    await flushPromises()
    expect(wrapper.find('[data-testid="add-instrument"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('添加投资标的')
    expect(wrapper.find('[data-testid="add-fund"]').exists()).toBe(false)
    expect(wrapper.find('[data-testid="create-instrument"]').exists()).toBe(false)
    await wrapper.find('[data-testid="add-instrument"]').trigger('click')
    await nextTick()
    await flushPromises()
    // 统一对话框打开（市场必选下拉在场）；弹窗内交互由 AddInstrumentModal.test.ts 覆盖
    expect(bodyQuery('[data-testid="add-instrument-market"]')).not.toBeNull()
  })

  it('添加成功：页面级回执 + 列表重拉（回到第 1 页）', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
    })
    const wrapper = mountBrowser()
    await flushPromises()
    const before = mockInvoke.mock.calls.filter(([cmd]) => cmd === 'list_instruments').length
    // 经组件 emit 驱动（对话框内查询/识别/兜底流程由 AddInstrumentModal.test.ts 覆盖）
    wrapper.findComponent({ name: 'AddInstrumentModal' }).vm.$emit(
      'added',
      '已添加投资标的：贵州茅台（600519 · 股票）最新价 123.45',
    )
    await flushPromises()
    const msg = wrapper.find('[data-testid="add-instrument-result"]')
    expect(msg.exists()).toBe(true)
    expect(msg.text()).toContain('已添加投资标的：贵州茅台')
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
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_instruments: () => Promise.resolve({ items, total: items.length }),      },
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
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_instruments: () => Promise.resolve({ items: [manualRow()], total: 1 }),
        delete_instrument: () => Promise.resolve(),      },
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
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_instruments: () => Promise.resolve({ items: [manualRow()], total: 1 }),
        delete_instrument: () =>
          Promise.reject({
            kind: 'Invalid',
            message: '该标的已有买卖流水，无法删除：可先删除相关交易后再试',
          }),      },
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
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_instruments: () => Promise.resolve({ items: rows, total: rows.length }),      },
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
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_instruments: () => Promise.resolve({ items: rows, total: rows.length }),      },
    })
    const wrapper = mountBrowser()
    await flushPromises()
    await wrapper.find('[data-testid="quote-稳稳地幸福"]').trigger('click')
    await nextTick()
    expect(document.body.textContent).toContain('录价 — 稳稳地幸福')
  })

  it('录价成功：页面级回执 + 列表零手动重拉（刷新由价格失效信号驱动）', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        record_manual_price: () =>
          Promise.resolve({ history_written: true, current_price_written: true }),      },
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
