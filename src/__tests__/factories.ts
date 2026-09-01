import { vi } from 'vitest'
import { registerToastSink, type ToastSink } from '@/composables/useLoadable'
import type {
  Account,
  Category,
  Currency,
  DashboardOverview,
  FinancialFreedomOverview,
  Holding,
  Instrument,
  ItemDailyTotal,
  Policy,
  PolicyStats,
  RealizedPnlSummary,
  Transaction,
} from '@/types'

/**
 * 组件/composable 测试的共享数据工厂与测试辅助（issue #110 审查：消除测试文件间重复）。
 * 各测试文件经 baseInvoke 辅助把这些对象接到对应 invoke 命令上。
 */

export const mockCurrencies: Currency[] = [
  { code: 'CNY', name: '人民币', symbol: '¥', decimal_places: 2 },
]

export function makeAccount(partial: Partial<Account> & { id: string }): Account {
  return {
    name: '证券账户A',
    type: 'investment',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    is_hidden: false,
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

export const mockAccounts: Account[] = [makeAccount({ id: 'acc-1' })]

export function makeCategory(partial: Partial<Category> & { id: string }): Category {
  return {
    name: partial.id,
    kind: 'expense',
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

export function makeInstrument(partial: Partial<Instrument> & { id: string }): Instrument {
  return {
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
    price_cents: null,
    invested: true,
    ...partial,
  }
}

export function makeHolding(partial: Partial<Holding> & { id: string; instrument_id: string }): Holding {
  return {
    account_id: 'acc-1',
    quantity: 100,
    cost_basis_cents: 120000,
    cost_currency_code: 'CNY',
    latest_price_cents: null,
    latest_price_currency_code: null,
    latest_nav_date: null,
    market_value_cents: null,
    unrealized_pnl_cents: null,
    updated_at: '2026-01-01T00:00:00Z',
    ...partial,
  }
}

/** h-1 有行情（价格/市值/未实现盈亏齐全），h-2 无行情（三项为 NULL） */
export const mockHoldings: Holding[] = [
  makeHolding({
    id: 'h-1',
    instrument_id: 'inst-1',
    quantity: 100,
    cost_basis_cents: 120000,
    latest_price_cents: 150000,
    latest_price_currency_code: 'CNY',
    market_value_cents: 150000,
    unrealized_pnl_cents: 30000,
  }),
  makeHolding({
    id: 'h-2',
    instrument_id: 'inst-2',
    quantity: 10,
    cost_basis_cents: 8000,
  }),
]

export const mockInstruments: Instrument[] = [
  makeInstrument({ id: 'inst-1' }),
  makeInstrument({ id: 'inst-2', symbol: '000001', name: '平安银行', market: 'sz' }),
]

/** dashboard_overview 返回值工厂（issue #143）：默认人民币本位币、净申 1234.56 元 */
/** 保单实体工厂（issue #360）：保单 store 与视图测试共用（消除本地复制）。 */
export function makePolicy(partial: Partial<Policy> & { id: string }): Policy {
  return {
    merchant_id: 'mer-1',
    policy_number: 'P2026-001',
    product_name: '重疾险',
    start_date: '2024-01-01',
    end_date: '2036-01-01',
    coverage_amount_cents: 30_000_000,
    coverage_currency_code: 'CNY',
    note: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

export function makePolicyStats(partial: Partial<PolicyStats> = {}): PolicyStats {
  return {
    policy_id: 'policy-1',
    native_currency: 'CNY',
    total_paid_native_cents: 600_000,
    total_inflow_native_cents: 50_000,
    next_charge_date: null,
    is_expired: false,
    ...partial,
  }
}

export function makeOverview(partial: Partial<DashboardOverview> = {}): DashboardOverview {
  return {
    native_currency: 'CNY',
    net_worth_cents: 123456,
    accounts_balance_cents: 100000,
    holdings_market_value_cents: 23456,
    ...partial,
  }
}

/** 交易行工厂：默认一笔 100 元人民币支出，覆写见 partial（id 必填）。 */
export function makeTransaction(partial: Partial<Transaction> & { id: string }): Transaction {
  return {
    kind: 'expense',
    amount_cents: 10000,
    currency_code: 'CNY',
    amount_native_cents: 10000,
    account_id: 'acc-1',
    to_account_id: null,
    category_id: null,
    merchant_id: null,
    refund_of_transaction_id: null,
    note: null,
    date: '2026-01-01',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    ...partial,
  }
}

/** item_daily_total 返回值工厂（issue #122）：默认人民币本位币、每天成本 123.45 元、3 件在用 */
export function makeItemDailyTotal(partial: Partial<ItemDailyTotal> = {}): ItemDailyTotal {
  return { native_currency: 'CNY', per_day_cents: 12345, item_count: 3, ...partial }
}

/** financial_freedom 返回值工厂（issue #344）：默认人民币本位币、自由度 7.5%
 * （可投资资产 5000 元 × 3% ÷ 年度预算 2 万）、覆盖 0.3 年 */
export function makeFinancialFreedom(
  partial: Partial<FinancialFreedomOverview> = {},
): FinancialFreedomOverview {
  return {
    ratio: 7.5,
    numerator_cents: 500000,
    denominator_cents: 2000000,
    coverage_years: 0.3,
    native_currency: 'CNY',
    ...partial,
  }
}

/** realized_pnl_summary 返回值工厂（issue #325）：默认全表汇总 300 元 */
export function makePnlSummary(partial: Partial<RealizedPnlSummary> = {}): RealizedPnlSummary {
  return {
    total_realized_pnl_cents: 30000,
    by_year: [{ year: '2026', realized_pnl_cents: 30000 }],
    by_account: [{ account_id: 'acc-1', account_name: '证券账户A', realized_pnl_cents: 30000 }],
    by_instrument: [
      { instrument_id: 'inst-1', symbol: '600000', name: '浦发银行', realized_pnl_cents: 30000 },
    ],
    details: [],
    ...partial,
  }
}

/** 假 toast sink：记录 error toast 调用（Loadable 默认策略经 sink 弹出，断言只看 sink 面） */
export function makeFakeSink(): ToastSink & { error: ReturnType<typeof vi.fn> } {
  return { error: vi.fn() }
}

/** 每用例复位 sink 为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰 */
export function resetToastSink(): void {
  registerToastSink({ error: () => {} })
}

/**
 * invoke mock 处理函数组装器：extra 优先，其次 defaults，均未命中则 reject「unexpected invoke」。
 * extra 中函数型 handler 以参数调用；其余当固定返回值。
 */
export function invokeHandler(
  defaults: Record<string, unknown>,
  extra?: Record<string, unknown>,
): (cmd: string) => unknown {
  return (cmd: string) => {
    const handler = extra && extra[cmd]
    if (typeof handler === 'function') return (handler as () => unknown)()
    if (handler !== undefined) return Promise.resolve(handler)
    if (cmd in defaults) return Promise.resolve(defaults[cmd])
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`))
  }
}
