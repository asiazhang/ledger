import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useReferenceStore } from '@/stores/reference'
import {
  formatCurrencyGroups,
  sumByCurrency,
  usePortfolioOverview,
} from '@/composables/usePortfolioOverview'
import {
  invokeHandler,
  mockAccounts,
  mockCurrencies,
  mockHoldings,
  mockInstruments,
} from './factories'

const mockInvoke = vi.mocked(invoke)

/** 默认 invoke mock：参考数据 + 持仓 + 持仓标的字典 */
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
      },
      extra,
    ),
  )
}

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  const store = useReferenceStore()
  await store.refresh()
})

describe('sumByCurrency 按币种汇总金额', () => {
  it('跳过空值并按币种累加、按币种代码排序', () => {
    expect(
      sumByCurrency([
        { currencyCode: 'CNY', cents: 100 },
        { currencyCode: 'HKD', cents: 50 },
        { currencyCode: 'CNY', cents: 200 },
        { currencyCode: 'USD', cents: null },
      ]),
    ).toEqual([
      { currencyCode: 'CNY', cents: 300 },
      { currencyCode: 'HKD', cents: 50 },
    ])
  })

  it('全部为空值时返回空数组', () => {
    expect(sumByCurrency([{ currencyCode: 'CNY', cents: null }])).toEqual([])
  })
})

describe('formatCurrencyGroups 分组合计展示文本（issue #145 首页复用）', () => {
  it('逐组格式化后以「 / 」连接', () => {
    const currencyMap = new Map(
      [
        ...mockCurrencies,
        { code: 'USD', name: '美元', symbol: '$', decimal_places: 2 },
      ].map((c) => [c.code, c]),
    )
    expect(
      formatCurrencyGroups(
        [
          { currencyCode: 'CNY', cents: 300 },
          { currencyCode: 'USD', cents: -500 },
        ],
        currencyMap,
      ),
    ).toBe('¥3 / -$5')
  })

  it('空分组降级为 -', () => {
    expect(formatCurrencyGroups([], new Map())).toBe('-')
  })
})

describe('usePortfolioOverview 盈亏页持仓概览数据层（issue #110）', () => {
  it('加载持仓并与持仓标的字典/账户信息拼装成行', async () => {
    const { rows, loading, refresh } = usePortfolioOverview()
    await refresh()
    expect(loading.value).toBe(false)
    expect(rows.value.length).toBe(2)
    const row1 = rows.value[0]
    expect(row1.symbol).toBe('600000')
    expect(row1.instrumentName).toBe('浦发银行')
    expect(row1.accountName).toBe('证券账户A')
    expect(row1.quantity).toBe(100)
    expect(row1.costBasisCents).toBe(120000)
    expect(row1.latestPriceCents).toBe(150000) // 现价为万分之一元刻度（ADR-0038）
    expect(row1.marketValueCents).toBe(150000)
    expect(row1.unrealizedPnlCents).toBe(30000)
    // 市值/未实现盈亏折算币种 = 账户币
    expect(row1.valueCurrencyCode).toBe('CNY')
  })

  it('查询持仓标的字典时携带 only_invested=true（与增量同步同口径）', async () => {
    const { refresh } = usePortfolioOverview()
    await refresh()
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === 'list_instruments')
    expect(call![1]).toMatchObject({ filter: { only_invested: true } })
  })

  it('总市值与未实现盈亏合计：排除无行情行，按账户币种汇总', async () => {
    const { totalMarketValueGroups, totalUnrealizedPnlGroups, refresh } =
      usePortfolioOverview()
    await refresh()
    // h-2 无行情 NULL 不计入；只有 h-1 计入
    expect(totalMarketValueGroups.value).toEqual([{ currencyCode: 'CNY', cents: 150000 }])
    expect(totalUnrealizedPnlGroups.value).toEqual([{ currencyCode: 'CNY', cents: 30000 }])
  })

  it('无持仓时行为明确：rows 为空、汇总为空数组，不报错', async () => {
    baseInvoke({ list_holdings: [], list_instruments: { items: [], total: 0 } })
    const { rows, loading, totalMarketValueGroups, totalUnrealizedPnlGroups, refresh } =
      usePortfolioOverview()
    await refresh()
    expect(rows.value).toEqual([])
    expect(totalMarketValueGroups.value).toEqual([])
    expect(totalUnrealizedPnlGroups.value).toEqual([])
    expect(loading.value).toBe(false)
  })
})
