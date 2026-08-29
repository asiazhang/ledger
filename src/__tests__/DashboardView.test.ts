import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import DashboardView from '@/views/DashboardView.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import { useReferenceStore } from '@/stores/reference'
import { useItemsStore } from '@/stores/items'
import { NProgress } from 'naive-ui'
import {
  invokeHandler,
  makeAccount,
  makeHolding,
  makeOverview,
  mockHoldings,
  mockInstruments,
} from './factories'
import type { Account, BudgetProgress, Currency, MonthlySummary } from '@/types'

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

// 净资产总览卡（issue #143）用例可按需覆写
const mockOverview = makeOverview()

// 首页新卡片用例可按需覆写；默认空集（无预算 → 预算卡隐藏、无当月行 → 三格为 0）
let mockMonthlySummary: MonthlySummary[] = []
let mockBudgetProgress: BudgetProgress[] = []

function setCurrentMonthSummary(summary: MonthlySummary) {
  const now = new Date()
  const monthKey = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}`
  mockMonthlySummary = [{ month: monthKey, ...summary }]
}

/** 在用物品每天成本合计（issue #122）用例可按需覆写 */
const mockItemDailyTotal = { native_currency: 'CNY', per_day_cents: 12345, item_count: 3 }

/** 默认 invoke mock：参考数据 + 持仓 + 持仓标的字典 + 本月收支/预算（extra 优先覆盖） */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: [],
        list_merchants: [],
        list_holdings: mockHoldings,
        list_instruments: { items: mockInstruments, total: mockInstruments.length },
        dashboard_overview: mockOverview,
        // 物品使用成本卡（issue #122）挂载时会创建物品 store（self-init 拉列表）
        list_items: [],
        item_daily_total: mockItemDailyTotal,
      },
      {
        // 函数型 handler 实时读取可变变量，#144 用例挂载前直接改写生效
        monthly_summary: () => mockMonthlySummary,
        budget_progress: () => mockBudgetProgress,
        ...extra,
      },
    ),
  )
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockMonthlySummary = []
  mockBudgetProgress = []
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

describe('DashboardView 净资产总览卡（issue #143）', () => {
  it('首页顶部呈现净资产总览卡：本位币单一主数字', async () => {
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="net-worth-card"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('净资产')
    // 123456 分 → ¥1234.56（本位币主数字，无各币种分项）
    expect(card.text()).toContain('¥1234.56')
  })

  it('命令报错（如缺汇率）时卡片显示提示文案而非空数字或崩溃', async () => {
    baseInvoke({
      dashboard_overview: () => Promise.reject(new Error('缺少 USD→CNY 汇率，无法折算')),
    })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="net-worth-card"]')
    expect(card.text()).toContain('净资产')
    expect(card.text()).toContain('缺少 USD→CNY 汇率，无法折算')
    // 不渲染空数字
    expect(card.text()).not.toContain('¥0')
  })
})

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

  it('无任何持仓时卡片保留，空态占位而非统计数字', async () => {
    baseInvoke({ list_holdings: [], list_instruments: { items: [], total: 0 } })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="investment-overview-card"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('暂无持仓')
    // 空态不渲染分组统计
    expect(card.text()).not.toContain('总市值')
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

  it('不再渲染逐账户余额卡片（逐账户明细归账户页，首页只呈现聚合全貌）', async () => {
    const wrapper = await mountView()
    expect(wrapper.text()).not.toContain('现金')
    // 不再查询账户余额
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd === 'list_account_balances')).toBe(false)
  })
})

describe('DashboardView 本月收支卡（issue #144）', () => {
  it('三格口径：收入=净收入、净支出=毛支出−退款、结余=收入−净支出', async () => {
    // income_net=100000、expense_gross=80000、refund=5000 → 净支出 75000、结余 25000
    setCurrentMonthSummary({ income_cents: 100000, expense_cents: 80000, refund_cents: 5000 })
    const wrapper = await mountView()
    const text = wrapper.text()
    expect(text).toContain('本月收支')
    expect(text).toContain('收入1000') // 净收入
    expect(text).toContain('净支出750') // 净支出（毛 800 − 退款 50）
    expect(text).toContain('结余250') // 结余
  })

  it('净支出与预算消耗、分类占比口径一致（退款冲减而非单列）', async () => {
    setCurrentMonthSummary({ income_cents: 0, expense_cents: 12345, refund_cents: 2345 })
    const wrapper = await mountView()
    // 净支出 10000 分 = expense_net 口径
    expect(wrapper.text()).toContain('净支出100')
  })

  it('当月无交易行时三格显示 0', async () => {
    mockMonthlySummary = [{ month: '1999-01', income_cents: 999, expense_cents: 888, refund_cents: 7 }]
    const wrapper = await mountView()
    expect(wrapper.text()).toContain('本月收支收入0净支出0结余0')
  })
})

describe('DashboardView 物品使用成本卡（issue #122）', () => {
  it('展示全部在用物品每天成本合计（默认币种）与在用件数', async () => {
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="item-daily-cost-card"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('全部在用物品每天成本合计')
    // 12345 分 → ¥123.45（默认币种，后端聚合结果直接展示）
    expect(card.text()).toContain('¥123.45/天')
    expect(card.text()).toContain('共 3 件在用物品')
  })

  it('无在用物品时空态占位而非 0 数字', async () => {
    baseInvoke({ item_daily_total: { native_currency: 'CNY', per_day_cents: 0, item_count: 0 } })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="item-daily-cost-card"]')
    expect(card.text()).toContain('暂无在用物品')
    expect(card.text()).not.toContain('/天')
  })

  it('聚合命令报错（如缺汇率）时显示提示文案而非空数字', async () => {
    baseInvoke({
      item_daily_total: () => Promise.reject(new Error('缺少 JPY→CNY 汇率，无法折算')),
    })
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="item-daily-cost-card"]')
    expect(card.text()).toContain('缺少 JPY→CNY 汇率，无法折算')
    // 不渲染空数字或合计
    expect(card.text()).not.toContain('¥')
  })

  it('物品写入失效（store version 变化）后自动重拉合计', async () => {
    let total = { native_currency: 'CNY', per_day_cents: 10000, item_count: 1 }
    baseInvoke({
      item_daily_total: () => Promise.resolve(total),
      list_items: [],
    })
    const wrapper = await mountView()
    expect(wrapper.find('[data-testid="item-daily-cost-card"]').text()).toContain('¥100/天')
    // 物品写入 → 物品 store 重拉（模拟 ledger:changed 路径）→ version 自增 → 合计跟随重拉
    total = { native_currency: 'CNY', per_day_cents: 30000, item_count: 2 }
    await useItemsStore().refresh()
    await flushPromises()
    const card = wrapper.find('[data-testid="item-daily-cost-card"]')
    expect(card.text()).toContain('¥300/天')
    expect(card.text()).toContain('共 2 件在用物品')
  })
})

describe('DashboardView 预算进度卡（issue #144）', () => {
  const progress = (over: boolean, spent: number, amount: number, name?: string): BudgetProgress => ({
    budget: {
      id: `b-${over ? 'over' : 'ok'}`,
      category_id: 'cat-1',
      period: 'monthly',
      amount_cents: amount,
      start_date: '2026-07-01',
      created_at: '2026-07-01T00:00:00Z',
      updated_at: '2026-07-01T00:00:00Z',
      version: 1,
      device_id: 'test',
      is_deleted: false,
    },
    category_name: name ?? (over ? '餐饮' : '交通'),
    spent_cents: spent,
    over_budget: over,
  })

  it('逐行渲染：分类名 + 进度条 + 已花/额度', async () => {
    mockBudgetProgress = [progress(false, 4000, 50000, '餐饮'), progress(false, 0, 10000, '交通')]
    const wrapper = await mountView()
    const text = wrapper.text()
    expect(text).toContain('预算进度')
    expect(text).toContain('餐饮')
    expect(text).toContain('交通')
    expect(text).toContain('40 / 500')
    expect(text).toContain('0 / 100')
    expect(wrapper.findComponent(NProgress).exists()).toBe(true)
  })

  it('超支行红色高亮：进度条 error 状态 + 超支标记', async () => {
    mockBudgetProgress = [progress(false, 4000, 50000, '交通'), progress(true, 60000, 50000, '餐饮')]
    const wrapper = await mountView()
    const bars = wrapper.findAllComponents(NProgress)
    expect(bars).toHaveLength(2)
    expect(bars[0].props('status')).toBe('success')
    expect(bars[1].props('status')).toBe('error')
    expect(wrapper.text()).toContain('超支')
    // 超支行金额红色高亮（NText type=error）
    expect(wrapper.text()).toContain('600 / 500')
  })

  it('无任何预算时卡片保留，空态占位而非逐行进度条', async () => {
    mockBudgetProgress = []
    const wrapper = await mountView()
    const card = wrapper.find('[data-testid="budget-progress-card"]')
    expect(card.exists()).toBe(true)
    expect(card.text()).toContain('未设置预算')
    expect(wrapper.findComponent(NProgress).exists()).toBe(false)
  })
})
