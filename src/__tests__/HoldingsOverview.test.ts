import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { NDataTable } from 'naive-ui'
import { nextTick } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import { applyLocale } from '@ledger/i18n'
import HoldingsOverview from '@/components/investments/HoldingsOverview.vue'
import {
  INSTRUMENT_SYNC_PROGRESS_EVENT,
  resetInstrumentInfoSyncForTest,
} from '@/composables/useInstrumentInfoSync'
import { captureListenHandlers } from '@ledger/test-support/listen-mock'
import { componentVm } from '@ledger/test-support/component-vm'
import { formatAmount, formatPrice } from '@ledger/money'
import {
  makeAccount,
  makeHolding,
  makeInstrument,
  mockHoldings,
  mockInstruments,
} from './factories'
import { refCurrencies } from '@ledger/test-support/reference-stubs'
import type { Account, Holding, Instrument } from '@ledger/types'

// 金额断言委托形态（issue #770）：期待值调同一 formatAmount/formatPrice 实现，
// 格式规则唯一归属其专测；现价列为价格刻度（ADR-0038）走 formatPrice
const cny = refCurrencies[0]
import {
  firePricesChanged,
  resetPricesChangedHandler,
} from './prices-changed-mock'

// 价格失效信号订阅基座 mock（issue #238 / ADR-0031 决策 3）：捕获订阅回调，
// 测试中手动触发模拟后端 emit；失败/零更新路径后端不 emit，即无重拉。
// 捕获/触发辅助收在 prices-changed-mock 共享（三个价格消费方测试同构）。
vi.mock('@/composables/usePricesChanged', async () => {
  const { capturePricesChangedHandler } = await import('./prices-changed-mock')
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  }
})

/** 默认布线 defaults 表：持仓 + 持仓标的字典 + 累计收益聚合 + 标的信息同步
 *（参考五命令走接缝规范兜底） */
const BASE_DEFAULTS = {
  list_holdings: mockHoldings,
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
  // 累计收益（issue #1077）：全账本按币种聚合，独立于持仓行（后端两腿相加）
  cumulative_pnl_summary: [{ currency_code: 'CNY', cumulative_pnl_cents: 48000 }],
  sync_instrument_info: { synced: 2, skipped: 0, message: '已同步 2 只，跳过 0 只' },
}

beforeEach(async () => {
  resetPricesChangedHandler()
  wireInvokeSeam({ defaults: BASE_DEFAULTS })
  const store = useReferenceStore()
  await store.refresh()
})

let wrapper: ReturnType<typeof mount> | undefined

async function cellText(colKey: string): Promise<string[]> {
  await nextTick()
  return wrapper!.findAll(`td[data-col-key="${colKey}"]`).map((c) => c.text())
}

describe('HoldingsOverview 当前持仓概览卡（issue #110）', () => {
  it('渲染总市值、持仓收益合计与累计收益（排除无行情行）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.text()).toContain('当前持仓')
    expect(wrapper.text()).toContain('总市值')
    expect(wrapper.text()).toContain(formatAmount(150000, cny))
    // 展示词逐字归一（issue #1077）：合计卡标签即「持仓收益」（账务口径仍为未实现盈亏）
    expect(wrapper.find('[data-testid="total-unrealized-pnl"]').text()).toBe(
      `持仓收益${formatAmount(30000, cny)}`,
    )
    // 展示词归一（issue #1077）：持仓页签不再残留「未实现盈亏」文案
    expect(wrapper.text()).not.toContain('未实现盈亏')
  })

  it('渲染持仓明细表列：标的/数量/成本/现价/市值/持仓收益', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const headers = wrapper.findAll('th').map((th) => th.text())
    for (const h of ['标的', '名称', '账户', '数量', '成本', '现价', '净值日期', '市值', '持仓收益']) {
      expect(headers).toContain(h)
    }
    // 行数据来自 mock（默认标的代码字母序，issue #902：000001 < 600000）
    expect(await cellText('symbol')).toEqual(['000001', '600000'])
    expect(await cellText('quantity')).toEqual(['10', '100'])
    expect(await cellText('cost_basis')).toEqual([formatAmount(8000, cny), formatAmount(120000, cny)])
    // 无行情行显示 -；现价列为价格刻度（formatPrice），其余列金额刻度（formatAmount）
    expect(await cellText('latest_price')).toEqual(['-', formatPrice(150000, cny)])
    expect(await cellText('market_value')).toEqual(['-', formatAmount(150000, cny)])
    expect(await cellText('unrealized_pnl')).toEqual(['-', formatAmount(30000, cny)])
  })

  it('合计区新增「累计收益」卡并按币种分组展示（issue #1077 接线负向条目，ADR-0087）', async () => {
    // 断言对准用户可观察结果：删除该卡的接线调用（NStatistic / testid）即找不到卡片、本用例变红。
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const card = wrapper.find('[data-testid="total-cumulative-pnl"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toBe(`累计收益${formatAmount(48000, cny)}`)
  })

  it('无持仓时显示空态', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { list_holdings: [], list_instruments: { items: [], total: 0 } },
    })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.find('.n-empty').exists()).toBe(true)
    expect(wrapper.text()).toContain('暂无持仓')
  })

  it('现价列展示基金净值 4 位小数（万分之一元刻度，ADR-0038）', async () => {
    const fundHolding = makeHolding({
      id: 'h-fund',
      instrument_id: 'inst-fund',
      quantity: 1000,
      cost_basis_cents: 123400,
      latest_price_cents: 12345,
      latest_price_currency_code: 'CNY',
      latest_nav_date: null,
      market_value_cents: 123450,
      unrealized_pnl_cents: 50,
    })
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_holdings: [fundHolding],
        list_instruments: {
          items: [
            ...mockInstruments,
            makeInstrument({ id: 'inst-fund', symbol: '000123', name: '净值保真基金' }),
          ],
          total: mockInstruments.length + 1,
        },
      },
    })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    // 现价 12345 万分之一元 = 1.2345 元，4 位小数无损展示；市值/未实现盈亏随行显形
    expect(await cellText('latest_price')).toEqual([formatPrice(12345, cny)])
    expect(await cellText('market_value')).toEqual([formatAmount(123450, cny)])
    expect(await cellText('unrealized_pnl')).toEqual([formatAmount(50, cny)])
  })

  it('净值日期独立成列：基金行有值、股票行显示 -；现价列恢复单行（#303 形态修订，issue #912）', async () => {
    const fundHolding = makeHolding({
      id: 'h-fund',
      instrument_id: 'inst-fund',
      latest_price_cents: 33480,
      latest_price_currency_code: 'CNY',
      latest_nav_date: '2026-01-30',
      market_value_cents: 334800,
      unrealized_pnl_cents: 50,
    })
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_holdings: [mockHoldings[0]!, fundHolding],
        list_instruments: {
          items: [
            ...mockInstruments,
            makeInstrument({ id: 'inst-fund', symbol: '110022', name: '易方达消费行业' }),
          ],
          total: mockInstruments.length + 1,
        },
      },
    })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    // 默认代码字母序：110022（基金）在前、600000（股票）在后
    // 净值日期列：仅基金行携带，股票行显示「-」
    expect(await cellText('nav_date')).toEqual(['2026-01-30', '-'])
    // 现价列恢复单行：只含价格本身，不再附净值日期小字
    expect(await cellText('latest_price')).toEqual([formatPrice(33480, cny), formatPrice(150000, cny)])
  })

  it('右上角「同步标的信息」按钮触发同步命令，反馈与标的页一致', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const btn = wrapper.find('[data-testid="sync-instrument-info"]')
    expect(btn.exists()).toBe(true)
    await btn.trigger('click')
    await flushPromises()
    expect(mockInvoke).toHaveBeenCalledWith('sync_instrument_info')
    // 同样的轻量反馈
    expect(wrapper.text()).toContain('已同步 2 只，跳过 0 只')
  })

  it('同步进行中按钮 loading', async () => {
    let resolveSync!: (v: unknown) => void
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: () =>
          new Promise((res) => {
            resolveSync = res
          }),
      },
    })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger('click')
    await nextTick()
    expect(wrapper.find('.n-button--loading').exists()).toBe(true)
    resolveSync({ synced: 2, skipped: 0, message: '已同步 2 只，跳过 0 只' })
    await flushPromises()
    expect(wrapper.find('.n-button--loading').exists()).toBe(false)
  })

  it('价格失效信号触发后重拉一次持仓（现价/市值随最新价刷新，issue #238）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length
    firePricesChanged()
    await flushPromises()
    const callsAfter = mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length
    expect(callsAfter).toBe(callsBefore + 1)
  })

  it('同步按钮只发起同步：点击不再直连重拉，重拉由信号驱动（样板移除）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger('click')
    await flushPromises()
    // 同步命令已发出，但点击路径自身不触发重拉——
    // 失败/零更新路径后端不 emit（ADR-0031 决策 2），即无重拉
    expect(mockInvoke).toHaveBeenCalledWith('sync_instrument_info')
    expect(mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length).toBe(callsBefore)
    // 信号到达才重拉
    firePricesChanged()
    await flushPromises()
    expect(mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length).toBe(callsBefore + 1)
  })

  it('同步进行中在卡顶展示确定进度条，完成后收起、结果消息接棒（issue #897）', async () => {
    let resolveSync!: (v: unknown) => void
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        sync_instrument_info: () =>
          new Promise((res) => {
            resolveSync = res
          }),
      },
    })
    resetInstrumentInfoSyncForTest()
    const handlers = captureListenHandlers()
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.find('[data-testid="instrument-sync-progress"]').exists()).toBe(false)

    await wrapper.find('[data-testid="sync-instrument-info"]').trigger('click')
    await nextTick()
    handlers.at(-1)!({ event: INSTRUMENT_SYNC_PROGRESS_EVENT, payload: { done: 37, total: 100 } })
    await flushPromises()

    const bar = wrapper.find('[data-testid="instrument-sync-progress"]')
    expect(bar.exists()).toBe(true)
    expect(bar.text()).toContain('同步标的信息 37/100')

    resolveSync({ synced: 100, skipped: 0, message: '已同步 100 只，跳过 0 只' })
    await flushPromises()
    expect(wrapper.find('[data-testid="instrument-sync-progress"]').exists()).toBe(false)
    expect(wrapper.text()).toContain('已同步 100 只，跳过 0 只')
  })

  it('同步失败显示错误消息', async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { sync_instrument_info: () => Promise.reject(new Error('网络错误')) },
    })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const callsBefore = mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length
    await wrapper.find('[data-testid="sync-instrument-info"]').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('同步失败：网络错误')
    // 失败路径后端不 emit（ADR-0031 决策 2），即无重拉
    expect(mockInvoke.mock.calls.filter(([c]) => c === 'list_holdings').length).toBe(callsBefore)
  })
})

// ---------------------------------------------------------------------------
// 持仓页签三维过滤排序（issue #902）：搜索（代码/名称/拼音，300ms 防抖）+
// 账户单选过滤 + 市值/未实现盈亏列头排序；合计随过滤子集更新、排序不影响；
// 缺价行金额列显示 "-"、排序恒排末尾；两态空态可区分。
// ---------------------------------------------------------------------------

/** 过滤夹具账户：现金（非投资，不进下拉）+ 两投资账户（HKD 本位币账户走 makeAccount 工厂） */
const FILTER_ACCOUNTS: Account[] = [
  makeAccount({ id: 'acc-1', name: '现金', type: 'cash' }),
  makeAccount({ id: 'acc-a', name: '证券A' }),
  makeAccount({ id: 'acc-b', name: '证券B', currency_code: 'HKD' }),
]

const FILTER_INSTRUMENTS: Instrument[] = [
  makeInstrument({ id: 'inst-1', symbol: '600000', name: '浦发银行', market: 'sh' }),
  makeInstrument({ id: 'inst-2', symbol: '000001', name: '平安银行', market: 'sz' }),
  makeInstrument({ id: 'inst-3', symbol: '00700', name: '腾讯控股', market: 'hk', currency_code: 'HKD' }),
  makeInstrument({ id: 'inst-4', symbol: 'AAPL', name: 'Apple', market: 'nasdaq', currency_code: 'USD' }),
]

/** 四行持仓：h-2 缺价（全 null 金额），h-3/h-4 在 acc-b（HKD/USD 本位币） */
const FILTER_HOLDINGS: Holding[] = [
  makeHolding({
    id: 'h-1',
    account_id: 'acc-a',
    instrument_id: 'inst-1',
    quantity: 100,
    cost_basis_cents: 120000,
    latest_price_cents: 150000,
    latest_price_currency_code: 'CNY',
    market_value_cents: 150000,
    unrealized_pnl_cents: 30000,
  }),
  makeHolding({ id: 'h-2', account_id: 'acc-a', instrument_id: 'inst-2', quantity: 10, cost_basis_cents: 8000 }),
  makeHolding({
    id: 'h-3',
    account_id: 'acc-b',
    instrument_id: 'inst-3',
    quantity: 1000,
    cost_basis_cents: 30000000,
    latest_price_cents: 200000,
    latest_price_currency_code: 'HKD',
    market_value_cents: 20000000,
    unrealized_pnl_cents: -500000,
  }),
  makeHolding({
    id: 'h-4',
    account_id: 'acc-b',
    instrument_id: 'inst-4',
    quantity: 10,
    cost_basis_cents: 400000,
    latest_price_cents: 500000,
    latest_price_currency_code: 'USD',
    market_value_cents: 500000,
    unrealized_pnl_cents: 10000,
  }),
]

const FILTER_DEFAULTS = {
  list_holdings: FILTER_HOLDINGS,
  list_instruments: { items: FILTER_INSTRUMENTS, total: FILTER_INSTRUMENTS.length },
  // 累计收益（issue #1077）：全账本口径，不随三维过滤收窄
  cumulative_pnl_summary: [{ currency_code: 'CNY', cumulative_pnl_cents: 26000 }],
  sync_instrument_info: { synced: 4, skipped: 0, message: '已同步 4 只，跳过 0 只' },
}

/** 搜索输入 → 行集合的同步桥：防抖窗口推进 + 微任务清空 */
async function typeSearch(wrapper: ReturnType<typeof mount>, text: string) {
  await wrapper.find('[data-testid="holdings-search"] input').setValue(text)
  vi.advanceTimersByTime(300)
  await flushPromises()
}

describe('HoldingsOverview 三维过滤排序（issue #902）', () => {
  beforeEach(async () => {
    vi.useFakeTimers()
    resetPricesChangedHandler()
    // 同步接缝状态是模块级共享单例：上一 describe 的失败反馈会跨实例残留，先复位
    resetInstrumentInfoSyncForTest()
    // 账户下拉夹具：投资账户谓词收口（acc-a/acc-b 进选项面，现金不进）
    wireInvokeSeam({ defaults: FILTER_DEFAULTS, overrides: { list_accounts: FILTER_ACCOUNTS } })
    const store = useReferenceStore()
    await store.refresh()
  })

  afterEach(() => {
    vi.useRealTimers()
    if (wrapper) {
      wrapper.unmount()
      wrapper = undefined
    }
  })

  it('默认态：全量行按标的代码字母序，合计为全量口径（缺价行不计入）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(await cellText('symbol')).toEqual(['000001', '00700', '600000', 'AAPL'])
    // CNY 组只含 h-1 的 150000（h-2 缺价跳过），HKD/USD 各自成组
    expect(wrapper.text()).toContain(formatAmount(150000, cny))
    expect(wrapper.text()).toContain(formatAmount(30000, cny))
    expect(wrapper.text()).toContain(formatAmount(20000000))
    expect(wrapper.text()).toContain(formatAmount(500000))
    // 缺价行金额列显示 "-"（空值语义保持）
    expect(await cellText('market_value')).toEqual(['-', formatAmount(20000000), formatAmount(150000, cny), formatAmount(500000)])
  })

  it('搜索按代码/名称/拼音命中，清空后恢复完整列表', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    // 代码子串
    await typeSearch(wrapper, '00700')
    expect(await cellText('symbol')).toEqual(['00700'])
    // 名称子串
    await typeSearch(wrapper, '浦发')
    expect(await cellText('symbol')).toEqual(['600000'])
    // 拼音首字母子序列（浦发银行 → pfyh）
    await typeSearch(wrapper, 'pfyh')
    expect(await cellText('symbol')).toEqual(['600000'])
    // 清空恢复完整列表
    await typeSearch(wrapper, '')
    expect(await cellText('symbol')).toEqual(['000001', '00700', '600000', 'AAPL'])
  })

  it('账户过滤单选生效，选「全部」（清除）恢复完整列表', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      'update:value',
      'acc-b',
    )
    await nextTick()
    expect(await cellText('symbol')).toEqual(['00700', 'AAPL'])
    // 清除回「全部」默认态
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      'update:value',
      null,
    )
    await nextTick()
    expect(await cellText('symbol')).toEqual(['000001', '00700', '600000', 'AAPL'])
  })

  it('市值列头升降序：缺价行恒排末尾；第三次点击回默认代码序', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    // 受控排序每次点击都重建表头：每次点击前重新查找 th（不用过期包装器）
    const clickMarketValue = async () => {
      await wrapper!.findAll('th').find((th) => th.text() === '市值')!.trigger('click')
      await nextTick()
    }
    // naive-ui 原生点击循环：首次点击落降序，缺价行仍恒排末尾（与方向无关）
    await clickMarketValue()
    expect(await cellText('symbol')).toEqual(['00700', 'AAPL', '600000', '000001'])
    // 升序：150000 → 500000 → 2000000，缺价行 h-2 末尾
    await clickMarketValue()
    expect(await cellText('symbol')).toEqual(['600000', 'AAPL', '00700', '000001'])
    // 第三态清除排序，回默认代码字母序
    await clickMarketValue()
    expect(await cellText('symbol')).toEqual(['000001', '00700', '600000', 'AAPL'])
  })

  it('持仓收益列头排序：盈利在前亏损在后，缺价行恒排末尾', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const pnlTh = wrapper.findAll('th').find((th) => th.text() === '持仓收益')!
    await pnlTh.trigger('click')
    await nextTick()
    // 降序：30000 → 10000 → -500000，缺价行末尾
    expect(await cellText('symbol')).toEqual(['600000', 'AAPL', '00700', '000001'])
    // 升序：盈利在后，缺价行仍末尾（表头重建后重新查找）
    await wrapper.findAll('th').find((th) => th.text() === '持仓收益')!.trigger('click')
    await nextTick()
    expect(await cellText('symbol')).toEqual(['00700', 'AAPL', '600000', '000001'])
  })

  it('合计随搜索与账户过滤子集更新；排序不改变合计', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    // 账户过滤后合计只含 acc-b 子集：h-3 + h-4 同折账户本位币（HKD），合为一组
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      'update:value',
      'acc-b',
    )
    await nextTick()
    const marketTotalText = () => wrapper!.find('[data-testid="total-market-value"]').text()
    expect(marketTotalText()).toContain(formatAmount(20500000))
    expect(marketTotalText()).not.toContain(formatAmount(150000, cny))
    // 排序只是重排行不是换口径：合计不动（降序 → 升序两轮后仍同值）
    await wrapper!.findAll('th').find((th) => th.text() === '市值')!.trigger('click')
    await nextTick()
    await wrapper.findAll('th').find((th) => th.text() === '市值')!.trigger('click')
    await nextTick()
    expect(marketTotalText()).toContain(formatAmount(20500000))
    // 搜索叠加在过滤之上：合计再收窄到命中行
    await typeSearch(wrapper, 'txkg')
    expect(marketTotalText()).toContain(formatAmount(20000000))
    expect(marketTotalText()).not.toContain(formatAmount(500000))
  })

  it('「没有持仓」与「筛选条件下无匹配」两种空态可区分', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    // 无匹配空态：有持仓但筛选不命中，文案非「暂无持仓」
    await typeSearch(wrapper, '999999')
    expect(wrapper.find('[data-testid="holdings-no-match"]').exists()).toBe(true)
    expect(wrapper.text()).toContain('筛选条件下无匹配持仓')
    expect(wrapper.text()).not.toContain('暂无持仓')
    // 清空后空态消失、完整列表恢复（过滤可逆）
    await typeSearch(wrapper, '')
    expect(wrapper.find('[data-testid="holdings-no-match"]').exists()).toBe(false)
    // 「没有持仓」空态：全库无持仓
    wireInvokeSeam({
      defaults: FILTER_DEFAULTS,
      overrides: { list_holdings: [], list_instruments: { items: [], total: 0 } },
    })
    wrapper.unmount()
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.text()).toContain('暂无持仓')
    expect(wrapper.text()).not.toContain('筛选条件下无匹配持仓')
    // 无持仓时过滤控件不渲染（无可过滤之列）
    expect(wrapper.find('[data-testid="holdings-search"]').exists()).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// 持仓页签客户端分页（issue #912）：页大小固定 20，切片由表格内置分页完成、
// 页码状态归 useHoldingsFilter（三维任一变化即翻页归零，卸载重挂回默认）；
// 合计/空态在切片前判定，与可见页无关。25 行夹具：第 21–25 行落第二页。
// ---------------------------------------------------------------------------

/** 分页夹具账户：22 行在 acc-1、3 行在 acc-2（账户过滤收窄后仍超一页） */
const PAGE_ACCOUNTS: Account[] = [
  makeAccount({ id: 'acc-page-1', name: '证券A' }),
  makeAccount({ id: 'acc-page-2', name: '证券B', currency_code: 'HKD' }),
]

/** 25 行持仓 + 字典：三位零填充代码序 = 数目字序，市值随序号递增（降序首行为 025） */
const PAGE_HOLDINGS: Holding[] = Array.from({ length: 25 }, (_, i) => {
  const code = String(i + 1).padStart(3, '0')
  return makeHolding({
    id: `ph-${code}`,
    account_id: i < 22 ? 'acc-page-1' : 'acc-page-2',
    instrument_id: `pinst-${code}`,
    quantity: 100,
    latest_price_cents: 100000 + i,
    latest_price_currency_code: 'CNY',
    market_value_cents: 10000000 + i,
    unrealized_pnl_cents: 1000 + i,
  })
})

const PAGE_INSTRUMENTS: Instrument[] = PAGE_HOLDINGS.map((h, i) =>
  makeInstrument({
    id: h.instrument_id,
    symbol: String(i + 1).padStart(3, '0'),
    name: `标的${String(i + 1).padStart(3, '0')}`,
  }),
)

const PAGE_DEFAULTS = {
  list_holdings: PAGE_HOLDINGS,
  list_instruments: { items: PAGE_INSTRUMENTS, total: PAGE_INSTRUMENTS.length },
  cumulative_pnl_summary: [],
  sync_instrument_info: { synced: 25, skipped: 0, message: '已同步 25 只，跳过 0 只' },
}

describe('HoldingsOverview 客户端分页（issue #912）', () => {
  beforeEach(async () => {
    vi.useFakeTimers()
    resetPricesChangedHandler()
    resetInstrumentInfoSyncForTest()
    wireInvokeSeam({ defaults: PAGE_DEFAULTS, overrides: { list_accounts: PAGE_ACCOUNTS } })
    const store = useReferenceStore()
    await store.refresh()
  })

  afterEach(async () => {
    vi.useRealTimers()
    if (wrapper) {
      wrapper.unmount()
      wrapper = undefined
    }
  })

  /** 点分页条页码项（.n-pagination-item 文本即页码；前后键无文本不参与匹配） */
  async function goToPage(page: number) {
    await wrapper!.findAll('.n-pagination-item').find((el) => el.text() === String(page))!.trigger('click')
    await nextTick()
  }

  it('超一页时分页条出现：首页 20 行，第二页余量 5 行，翻回恢复', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.find('.n-pagination').exists()).toBe(true)
    expect(await cellText('symbol')).toHaveLength(20)
    expect((await cellText('symbol'))[0]).toBe('001')
    await goToPage(2)
    expect(await cellText('symbol')).toEqual(['021', '022', '023', '024', '025'])
    await goToPage(1)
    expect(await cellText('symbol')).toHaveLength(20)
    expect((await cellText('symbol'))[0]).toBe('001')
  })

  it('合计为过滤子集全量口径，翻页不改变合计', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const totalText = () => wrapper!.find('[data-testid="total-market-value"]').text()
    const before = totalText()
    await goToPage(2)
    expect(totalText()).toBe(before)
  })

  it('筛选变化翻页归零：第二页时改账户过滤，回新行集第一页', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    await goToPage(2)
    expect(await cellText('symbol')).toHaveLength(5)
    // 账户过滤收窄到 22 行（>20）：归零后第一页显示 20 行；若停留第 2 页只会看到 2 行
    componentVm(wrapper.findComponent('[data-testid="holdings-account-filter"]')).$emit(
      'update:value',
      'acc-page-1',
    )
    await nextTick()
    expect(await cellText('symbol')).toHaveLength(20)
    expect((await cellText('symbol'))[0]).toBe('001')
  })

  it('排序变化翻页归零：第二页时点市值列头，回排序后第一页', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    await goToPage(2)
    expect(await cellText('symbol')).toHaveLength(5)
    // 受控排序重排行集 + 页码归零：显示市值降序前 20 行（首行市值最大 = 025）
    await wrapper!.findAll('th').find((th) => th.text() === '市值')!.trigger('click')
    await nextTick()
    expect(await cellText('symbol')).toHaveLength(20)
    expect((await cellText('symbol'))[0]).toBe('025')
  })

  it('搜索变化翻页归零：第二页时搜索命中全量，回新行集第一页', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    await goToPage(2)
    expect(await cellText('symbol')).toHaveLength(5)
    // 「标的」命中全部 25 行名（>20）：归零后第一页 20 行；若停留第 2 页只剩 5 行
    await typeSearch(wrapper, '标的')
    expect(await cellText('symbol')).toHaveLength(20)
    expect((await cellText('symbol'))[0]).toBe('001')
  })

  it('页码与三维状态同瞬态：卸载重挂回第一页（离开视图回默认）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    await goToPage(2)
    expect(await cellText('symbol')).toHaveLength(5)
    wrapper.unmount()
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(await cellText('symbol')).toHaveLength(20)
    expect(wrapper.findAll('.n-pagination-item--active').map((el) => el.text())).toEqual(['1'])
  })

  it('单页行集不出现分页条（4 行三维过滤夹具回归不变）', async () => {
    wireInvokeSeam({ defaults: FILTER_DEFAULTS, overrides: { list_accounts: FILTER_ACCOUNTS } })
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.find('.n-pagination').exists()).toBe(false)
    expect(await cellText('symbol')).toEqual(['000001', '00700', '600000', 'AAPL'])
  })

  it('净值日期列名 i18n：en-US 为 NAV Date（用例后还原 zh-CN）', async () => {
    await applyLocale('en-US')
    try {
      wrapper = mount(HoldingsOverview)
      await flushPromises()
      expect(wrapper.findAll('th').map((th) => th.text())).toContain('NAV Date')
    } finally {
      await applyLocale('zh-CN')
    }
  })
})

describe('HoldingsOverview 表格列形态（词汇表「表格列形态」约定）', () => {
  it('数值列右对齐 + 等宽数字；名称列唯一弹性（无固定宽、minWidth + ellipsis）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const columns = wrapper.findComponent(NDataTable).props('columns') as unknown as Array<Record<string, unknown>>
    const byKey = (key: string) => columns.find((c) => c.key === key)!
    for (const key of ['quantity', 'cost_basis', 'latest_price', 'nav_date', 'market_value', 'unrealized_pnl']) {
      expect(byKey(key).align, key).toBe('right')
      expect(byKey(key).className, key).toBe('tabular-nums')
    }
    const nameCol = byKey('instrumentName')
    expect(nameCol.width).toBeUndefined()
    expect(nameCol.minWidth).toBeGreaterThan(0)
    expect(nameCol.ellipsis).toBeTruthy()
    // 其余短内容列维持固定宽（scrollX = 固定列宽总和的前提）
    for (const key of ['symbol', 'accountName', 'nav_date']) {
      expect(typeof byKey(key).width, key).toBe('number')
    }
  })
})
