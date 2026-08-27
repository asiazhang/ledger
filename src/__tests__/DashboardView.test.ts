import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import DashboardView from '@/views/DashboardView.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import { useReferenceStore } from '@/stores/reference'
import {
  invokeHandler,
  makeAccount,
  makeHolding,
  mockHoldings,
  mockInstruments,
} from './factories'
import type { Account, AccountBalance, Currency } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
  { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
]

const mockAccounts: Account[] = [
  {
    id: 'acc-1',
    name: '现金',
    type: 'cash',
    currency_code: 'CNY',
    initial_balance_cents: 10000,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

const mockBalances: AccountBalance[] = [
  { account: mockAccounts[0], balance_cents: 10000 },
]

/** 默认 invoke mock：参考数据 + 余额 + 持仓 + 持仓标的字典（extra 优先覆盖） */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: [],
        list_account_balances: mockBalances,
        list_holdings: mockHoldings,
        list_instruments: { items: mockInstruments, total: mockInstruments.length },
      },
      extra,
    ),
  )
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  localStorage.clear()
  const store = useReferenceStore()
  await store.ensureFresh()
})

async function mountView() {
  const wrapper = mount(DashboardView)
  await flushPromises()
  return wrapper
}

describe('DashboardView 投资概览卡（issue #145）', () => {
  it('有持仓时展示按币种分组的总市值与未实现盈亏合计，无行情行不以零计入', async () => {
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="investment-overview-card"]')
    expect(card.exists()).toBe(true)
    // h-1 有行情计入：150000 分 → ¥1500；30000 分 → ¥300（4 位整数不触发万分位分组）
    // h-2 无行情（NULL）不计入：合计中不出现 ¥0
    expect(card.text()).toContain('总市值')
    expect(card.text()).toContain('¥1500')
    expect(card.text()).toContain('未实现盈亏合计')
    expect(card.text()).toContain('¥300')
    expect(card.text()).not.toContain('¥0')
  })

  it('多币种持仓按币种分组展示，组间以「 / 」连接', async () => {
    const usdAccount = makeAccount({ id: 'acc-2', name: '美股账户', currency_code: 'USD' })
    const usdHolding = makeHolding({
      id: 'h-3',
      instrument_id: 'inst-2',
      account_id: 'acc-2',
      cost_currency_code: 'USD',
      market_value_cents: 3000,
      unrealized_pnl_cents: -500,
    })
    baseInvoke({
      list_holdings: [mockHoldings[0], usdHolding],
      list_accounts: [mockAccounts[0], usdAccount],
    })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="investment-overview-card"]')
    // 币种代码排序：CNY 在前、USD 在后
    expect(card.text()).toContain('¥1500 / $30')
    expect(card.text()).toContain('¥300 / -$5')
  })

  it('无任何持仓时整卡隐藏', async () => {
    baseInvoke({ list_holdings: [], list_instruments: { items: [], total: 0 } })
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="investment-overview-card"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('投资概览')
  })

  it('有持仓但全部无行情时空值分支：合计统计精确降级为「总市值-」', async () => {
    baseInvoke({
      list_holdings: [mockHoldings[1]],
      list_instruments: { items: [mockInstruments[1]], total: 1 },
    })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="investment-overview-card"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('总市值')
    // 精确锁定降级文本：NStatistic 渲染 label + value 连排
    expect(card.find('[data-testid="dashboard-total-market-value"]').text()).toBe('总市值-')
    expect(card.find('[data-testid="dashboard-total-unrealized-pnl"]').text()).toBe(
      '未实现盈亏合计-',
    )
  })
})

describe('DashboardView 快速记账与最近交易移除（issue #141）', () => {
  it('不再渲染快速记账表单（TransactionForm）', async () => {
    const wrapper = await mountView()
    expect(wrapper.findComponent(TransactionForm).exists()).toBe(false)
    expect(wrapper.text()).not.toContain('快速记账')
  })

  it('不再渲染最近交易列表', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).not.toContain('最近交易')
    // 不再查询交易列表
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'list_transactions')).toBe(false)
  })

  it('账户余额卡片保留（仪表盘改造前的既有内容）', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('现金')
    // 金额展示裁剪尾零（issue #148）：10000 分 → ¥100
    expect(wrapper.text()).toContain('¥100')
  })
})
