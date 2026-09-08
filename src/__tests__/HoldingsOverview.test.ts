import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { nextTick } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import HoldingsOverview from '@/components/investments/HoldingsOverview.vue'
import { formatAmount, formatPrice } from '@/utils/money'
import {
  makeHolding,
  makeInstrument,
  mockHoldings,
  mockInstruments,
} from './factories'
import { refCurrencies } from './helpers/reference-stubs'

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

/** 默认布线 defaults 表：持仓 + 持仓标的字典 + 标的信息同步（参考五命令走接缝规范兜底） */
const BASE_DEFAULTS = {
  list_holdings: mockHoldings,
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
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
  it('渲染总市值与未实现盈亏合计（排除无行情行）', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    expect(wrapper.text()).toContain('当前持仓')
    expect(wrapper.text()).toContain('总市值')
    expect(wrapper.text()).toContain(formatAmount(150000, cny))
    expect(wrapper.text()).toContain('未实现盈亏合计')
    expect(wrapper.text()).toContain(formatAmount(30000, cny))
  })

  it('渲染持仓明细表列：标的/数量/成本/现价/市值/未实现盈亏', async () => {
    wrapper = mount(HoldingsOverview)
    await flushPromises()
    const headers = wrapper.findAll('th').map((th) => th.text())
    for (const h of ['标的', '名称', '账户', '数量', '成本', '现价', '市值', '未实现盈亏']) {
      expect(headers).toContain(h)
    }
    // 行数据来自 mock
    expect(await cellText('symbol')).toEqual(['600000', '000001'])
    expect(await cellText('quantity')).toEqual(['100', '10'])
    expect(await cellText('cost_basis')).toEqual([formatAmount(120000, cny), formatAmount(8000, cny)])
    // 无行情行显示 -；现价列为价格刻度（formatPrice），其余列金额刻度（formatAmount）
    expect(await cellText('latest_price')).toEqual([formatPrice(150000, cny), '-'])
    expect(await cellText('market_value')).toEqual([formatAmount(150000, cny), '-'])
    expect(await cellText('unrealized_pnl')).toEqual([formatAmount(30000, cny), '-'])
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

  it('基金行现价下方展示净值日期（现价对应哪天的净值，#303），股票行不展示', async () => {
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
    const cells = await cellText('latest_price')
    // 股票行（无净值日期）只有价格
    expect(cells[0]).toBe(formatPrice(150000, cny))
    expect(cells[1]).toContain(formatPrice(33480, cny))
    // 基金行现价下方展示净值日期
    expect(cells[1]).toContain('净值 2026-01-30')
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
