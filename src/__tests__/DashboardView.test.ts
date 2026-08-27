import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import DashboardView from '@/views/DashboardView.vue'
import TransactionForm from '@/components/TransactionForm.vue'
import { useReferenceStore } from '@/stores/reference'
import { NProgress } from 'naive-ui'
import type { Account, AccountBalance, BudgetProgress, Currency, MonthlySummary } from '@/types'

const mockInvoke = vi.mocked(invoke)

const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
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

// 首页新卡片用例可按需覆写；默认空集（无预算 → 预算卡隐藏、无当月行 → 三格为 0）
let mockMonthlySummary: MonthlySummary[] = []
let mockBudgetProgress: BudgetProgress[] = []

function setCurrentMonthSummary(summary: MonthlySummary) {
  const now = new Date()
  const monthKey = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}`
  mockMonthlySummary = [{ month: monthKey, ...summary }]
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  mockMonthlySummary = []
  mockBudgetProgress = []
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'list_currencies') return Promise.resolve(mockCurrencies)
    if (cmd === 'list_accounts') return Promise.resolve(mockAccounts)
    if (cmd === 'list_categories') return Promise.resolve([])
    if (cmd === 'list_account_balances') return Promise.resolve(mockBalances)
    if (cmd === 'monthly_summary') return Promise.resolve(mockMonthlySummary)
    if (cmd === 'budget_progress') return Promise.resolve(mockBudgetProgress)
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

  it('无任何预算时整卡隐藏，不出空区块', async () => {
    mockBudgetProgress = []
    const wrapper = await mountView()
    expect(wrapper.text()).not.toContain('预算进度')
  })
})
