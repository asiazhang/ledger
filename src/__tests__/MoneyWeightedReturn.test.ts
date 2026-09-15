import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { mockInvoke, wireInvokeSeam } from '@ledger/test-support/invoke-mock'
import { componentVm } from '@ledger/test-support/component-vm'
import { mountWithDialog } from '@ledger/test-support/mount'
import { firePricesChanged, resetPricesChangedHandler } from './prices-changed-mock'
import { makeMwrSummary, makePnlSummary } from './factories'
import RealizedPnlPanel from '@/components/investments/RealizedPnlPanel.vue'
import HoldingsOverview from '@/components/investments/HoldingsOverview.vue'

// 价格失效信号订阅 mock（同 HoldingsOverview.test.ts 基座）：捕获订阅回调，
// 用例手动触发模拟后端 emit。
vi.mock('@/composables/usePricesChanged', async () => {
  const { capturePricesChangedHandler } = await import('./prices-changed-mock')
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  }
})

/** 持仓概览 defaults：两行持仓（h-1 有行情、h-2 缺价），收益率三态由 overrides 表达 */
const HOLDINGS_DEFAULTS = {
  list_holdings: [
    {
      id: 'h-1',
      account_id: 'acc-1',
      instrument_id: 'inst-1',
      quantity: 100,
      cost_basis_cents: 120000,
      cost_currency_code: 'CNY',
      latest_price_cents: 150000,
      latest_price_currency_code: 'CNY',
      latest_nav_date: null,
      market_value_cents: 150000,
      unrealized_pnl_cents: 30000,
      updated_at: '2026-01-01T00:00:00Z',
    },
    {
      id: 'h-2',
      account_id: 'acc-1',
      instrument_id: 'inst-2',
      quantity: 10,
      cost_basis_cents: 8000,
      cost_currency_code: 'CNY',
      latest_price_cents: null,
      latest_price_currency_code: null,
      latest_nav_date: null,
      market_value_cents: null,
      unrealized_pnl_cents: null,
      updated_at: '2026-01-01T00:00:00Z',
    },
  ],
  list_instruments: {
    items: [
      { id: 'inst-1', symbol: '600000', name: '浦发银行', price_channel: 'quote' },
      { id: 'inst-2', symbol: '000001', name: '平安银行', price_channel: 'quote' },
    ],
    total: 2,
  },
  cumulative_pnl_summary: [{ currency_code: 'CNY', cumulative_pnl_cents: 48000 }],
  sync_instrument_info: { synced: 0, skipped: 0, message: '' },
}

beforeEach(async () => {
  resetPricesChangedHandler()
  // 参考 store 预载走接缝 opt-in 参数（五个 list 命令由桩层规范夹具兑底）；
  // 用例级差异再叠 wireInvokeSeam 覆盖（先例：InvestmentsView.test.ts）。
  await wireInvokeSeam({ defaults: HOLDINGS_DEFAULTS, refreshReferenceStores: true }).ready
})

afterEach(() => {
  resetPricesChangedHandler()
})

function mwrCellTexts(
  wrapper: ReturnType<typeof mount>,
  colKey: 'rate' | 'mwr' = 'rate',
): string[] {
  return wrapper.findAll(`td[data-col-key="${colKey}"]`).map((c) => c.text())
}

describe('资金加权收益率前端接线（issue #1195 / ADR-0115）', () => {
  it('持仓页签收益率列：可计算行渲染带色百分数，缺价行「-」，无解行「无法计算」', async () => {
    // 三态一次布线：inst-1 可计算（+10%）、inst-2 缺价（行缺席 →「-」）、
    // 追加一行无解（rate null →「无法计算」）。
    wireInvokeSeam({
      defaults: {
        ...HOLDINGS_DEFAULTS,
        money_weighted_return_summary: makeMwrSummary({
          by_instrument: [
            {
              account_id: 'acc-1',
              instrument_id: 'inst-1',
              currency_code: 'CNY',
              basis: 'annualized',
              rate: 0.1,
            },
            {
              account_id: 'acc-1',
              instrument_id: 'inst-2',
              currency_code: 'CNY',
              basis: 'annualized',
              rate: null,
            },
          ],
        }),
      },
    })
    const wrapper = mountWithDialog(HoldingsOverview)
    await flushPromises()

    // 调用事实：命令被拉取（双断言之调用面）
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'money_weighted_return_summary')).toBe(true)
    // 效果面：列头在场 + 三态逐行分流
    const headers = wrapper.findAll('th').map((th) => th.text())
    expect(headers).toContain('资金加权收益率')
    const cells = wrapper.findAll('td[data-col-key="mwr"]').map((c) => c.text())
    expect(cells).toHaveLength(2)
    // 行序按标的代码字母序（issue #902 默认序）：inst-2（000001，无解）在前、
    // inst-1（600000，+10%）在后。
    expect(cells[0]).toBe('无法计算')
    expect(cells[1]).toBe('+10.00%')

    // 另一行缺席形态（后端不返回该对 → 前端「-」）由缺省布线表达：
    wireInvokeSeam({
      defaults: HOLDINGS_DEFAULTS,
      overrides: {
        money_weighted_return_summary: makeMwrSummary({
          by_instrument: [
            {
              account_id: 'acc-1',
              instrument_id: 'inst-1',
              currency_code: 'CNY',
              basis: 'annualized',
              rate: 0.1,
            },
          ],
        }),
      },
    })
    const wrapper2 = mountWithDialog(HoldingsOverview)
    await flushPromises()
    const cells2 = wrapper2.findAll('td[data-col-key="mwr"]').map((c) => c.text())
    // inst-2 无收益率行（缺价跳过）→「-」；inst-1 可计算。
    expect(cells2).toEqual(['-', '+10.00%'])
    wrapper.unmount()
    wrapper2.unmount()
  })

  it('期初存量标的：按行口径标「未年化」，年化标的保持原样（issue #1343）', async () => {
    // 后端为含期初存量的行带 basis=cumulative（3.16% 是累计收益 ÷ 累计投入，
    // 不是年化）；展示层必须标出口径，否则会被读成年化值。
    wireInvokeSeam({
      defaults: {
        ...HOLDINGS_DEFAULTS,
        money_weighted_return_summary: makeMwrSummary({
          by_instrument: [
            {
              account_id: 'acc-1',
              instrument_id: 'inst-1',
              currency_code: 'CNY',
              basis: 'cumulative',
              rate: 0.0316,
            },
            {
              account_id: 'acc-1',
              instrument_id: 'inst-2',
              currency_code: 'CNY',
              basis: 'annualized',
              rate: 0.1,
            },
          ],
        }),
      },
    })
    const wrapper = mountWithDialog(HoldingsOverview)
    await flushPromises()

    // 行序按标的代码字母序（issue #902 默认序）：inst-2（000001，年化）在前、
    // inst-1（600000，未年化）在后。
    expect(mwrCellTexts(wrapper, 'mwr')).toEqual(['+10.00%', '+3.16%（未年化）'])
    wrapper.unmount()
  })

  it('盈亏页资金加权收益率卡：账户行 + 全账行（按币种分组），无解显式标注', async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary({
          by_account: [
            { account_id: 'acc-1', account_name: '证券账户A', currency_code: 'CNY', rate: 0.1 },
          ],
          total: [
            { currency_code: 'CNY', rate: 0.08 },
            { currency_code: 'USD', rate: null },
          ],
        }),
      },
    })
    const wrapper = mountWithDialog(RealizedPnlPanel)
    await flushPromises()

    expect(wrapper.text()).toContain('资金加权收益率')
    // 调用事实 + 效果面（双断言）
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'money_weighted_return_summary')).toBe(true)
    expect(mwrCellTexts(wrapper)).toEqual(['+10.00%', '+8.00%', '无法计算'])
    const scopeTexts = wrapper.findAll('td[data-col-key="scope"]').map((c) => c.text())
    expect(scopeTexts).toEqual(['证券账户A', '全账', '全账'])
    const currencyTexts = wrapper.findAll('td[data-col-key="currency_code"]').map((c) => c.text())
    expect(currencyTexts).toEqual(['CNY', 'CNY', 'USD'])
    wrapper.unmount()
  })

  it('盈亏页账户筛选收窄账户行，全账行保持全账本口径', async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary({
          by_account: [
            { account_id: 'acc-1', account_name: '证券账户A', currency_code: 'CNY', rate: 0.1 },
            { account_id: 'acc-2', account_name: '基金账户B', currency_code: 'CNY', rate: -0.05 },
          ],
        }),
      },
    })
    const wrapper = mountWithDialog(RealizedPnlPanel)
    await flushPromises()
    expect(await mwrCellTexts(wrapper)).toEqual(['+10.00%', '-5.00%', '+10.00%'])

    // 账户下拉选 acc-1：账户行只剩证券账户A，全账行不动（与累计收益合计同一先例）
    const select = wrapper.findComponent({ name: 'PinyinSelect' })
    componentVm(select).$emit('update:value', 'acc-1')
    await nextTick()
    const scopeTexts = wrapper.findAll('td[data-col-key="scope"]').map((c) => c.text())
    expect(scopeTexts).toEqual(['证券账户A', '全账'])
    wrapper.unmount()
  })

  it('价格失效信号驱动重拉（期末市值随行情变动）', async () => {
    wireInvokeSeam({
      defaults: {
        realized_pnl_summary: makePnlSummary(),
        money_weighted_return_summary: makeMwrSummary(),
      },
    })
    const wrapper = mountWithDialog(RealizedPnlPanel)
    await flushPromises()
    const callsBefore = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === 'money_weighted_return_summary',
    ).length
    expect(callsBefore).toBeGreaterThanOrEqual(1)

    firePricesChanged()
    await flushPromises()
    const callsAfter = mockInvoke.mock.calls.filter(
      ([cmd]) => cmd === 'money_weighted_return_summary',
    ).length
    expect(callsAfter).toBeGreaterThan(callsBefore)
    wrapper.unmount()
  })
})
