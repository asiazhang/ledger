import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import DashboardView from '@/views/DashboardView.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import { useReferenceStore } from '@/stores/reference'
import { makeAccount, makeHolding, makeInstrument } from './factories'
import type { Account, AccountBalance, Currency, Holding, Instrument } from '@/types'

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

const mockInstruments: Instrument[] = [
  makeInstrument({ id: 'inst-1' }),
  makeInstrument({ id: 'inst-2', symbol: '000001', name: '平安银行', market: 'sz' }),
]

/** 默认持仓：h-1 有行情（折算到 CNY 账户），h-2 无行情（三项 NULL，不计入合计） */
const mockHoldings: Holding[] = [
  makeHolding({
    id: 'h-1',
    instrument_id: 'inst-1',
    quantity: 100,
    cost_basis_cents: 120000,
    latest_price_cents: 1500,
    latest_price_currency_code: 'CNY',
    market_value_cents: 150000,
    unrealized_pnl_cents: 30000,
  }),
  makeHolding({ id: 'h-2', instrument_id: 'inst-2', quantity: 10, cost_basis_cents: 8000 }),
]

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
    if (cmd === 'list_holdings') return Promise.resolve(mockHoldings)
    if (cmd === 'list_instruments')
      return Promise.resolve({ items: mockInstruments, total: mockInstruments.length })
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  })
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
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve([mockAccounts[0]!, usdAccount])
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
      if (cmd === 'list_holdings') return Promise.resolve([mockHoldings[0]!, usdHolding])
      if (cmd === 'list_instruments')
        return Promise.resolve({ items: mockInstruments, total: mockInstruments.length })
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="investment-overview-card"]')
    // 币种代码排序：CNY 在前、USD 在后
    expect(card.text()).toContain('¥1500 / $30')
    expect(card.text()).toContain('¥300 / -$5')
  })

  it('无任何持仓时整卡隐藏', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
      if (cmd === 'list_holdings') return Promise.resolve([])
      if (cmd === 'list_instruments') return Promise.resolve({ items: [], total: 0 })
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="investment-overview-card"]').exists()).toBe(false)
    expect(wrapper.text()).not.toContain('投资概览')
  })

  it('有持仓但全部无行情时空值分支：合计展示为 -', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
      if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
      if (cmd === 'list_categories') return Promise.resolve([])
      if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
      if (cmd === 'list_holdings') return Promise.resolve([mockHoldings[1]!])
      if (cmd === 'list_instruments')
        return Promise.resolve({ items: [mockInstruments[1]!], total: 1 })
      return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
    })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="investment-overview-card"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('总市值')
    expect(card.text()).toContain('-')
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
