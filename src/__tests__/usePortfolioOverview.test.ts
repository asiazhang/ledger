import { describe, it, expect, beforeEach } from 'vitest'
import { mockInvoke } from './helpers/invoke-mock'
import { mount, flushPromises } from '@vue/test-utils'
import { defineComponent } from 'vue'
import { setActivePinia, createPinia } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import { registerToastSink } from '@/composables/useLoadable'
import {
  formatCurrencyGroups,
  sumByCurrency,
  usePortfolioOverview,
} from '@/composables/usePortfolioOverview'
import {
  invokeHandler,
  makeFakeSink,
  mockAccounts,
  mockCurrencies,
  mockHoldings,
  mockInstruments,
  resetToastSink,
} from './factories'


/** 默认 invoke mock：参考数据 + 持仓 + 持仓标的字典 */
function baseInvoke(extra?: Record<string, unknown>) {
  mockInvoke.mockImplementation(
    invokeHandler(
      {
        list_currencies: mockCurrencies,
        list_accounts: mockAccounts,
        list_categories: [],
        list_merchants: [],
        list_insurers: [],
        list_holdings: mockHoldings,
        list_instruments: { items: mockInstruments, total: mockInstruments.length },
      },
      extra,
    ),
  )
}

/** 宿主组件：模拟盈亏页/首页在 setup 内使用 composable（onMounted 自动首刷时序留在薄壳内） */
const Host = defineComponent({
  setup() {
    return { shell: usePortfolioOverview() }
  },
  template: '<div />',
})

beforeEach(async () => {
  setActivePinia(createPinia())
  mockInvoke.mockReset()
  baseInvoke()
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink()
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

  it('净值日期透传到行（基金现价对应哪天的净值，#303）', async () => {
    const { rows, refresh } = usePortfolioOverview()
    await refresh()
    // 默认夹具为股票行：latest_nav_date 为 null
    expect(rows.value[0]!.latestNavDate).toBeNull()
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

describe('usePortfolioOverview 失败治愈（issue #324 Loadable 薄壳化）', () => {
  it('刷新失败不向调用方抛出：error 置位、loading 收尾、rows 保持原值不清空', async () => {
    const { rows, loading, error, refresh } = usePortfolioOverview()
    await refresh()
    expect(rows.value.length).toBe(2)

    baseInvoke({ list_holdings: () => Promise.reject(new Error('数据库文件已锁定')) })
    await expect(refresh()).resolves.not.toThrow()
    expect(loading.value).toBe(false)
    expect(error.value).toBe('数据库文件已锁定')
    expect(rows.value.length).toBe(2)
  })

  it('失败弹默认 toast（serde 对象错误归一取 message），成功不弹——error 状态与 toast 双通道共存', async () => {
    const sink = makeFakeSink()
    registerToastSink(sink)
    baseInvoke({
      list_holdings: () => Promise.reject({ kind: 'db', message: '持仓查询失败' }),
    })
    const { error, refresh } = usePortfolioOverview()
    await refresh()
    expect(error.value).toBe('持仓查询失败')
    expect(sink.error).toHaveBeenCalledTimes(1)
    expect(sink.error).toHaveBeenCalledWith('持仓查询失败')

    baseInvoke()
    await refresh()
    expect(sink.error).toHaveBeenCalledTimes(1)
  })

  it('失败后重试成功：error 清零、rows 重新装配（error 是唯一成败判据）', async () => {
    baseInvoke({ list_holdings: () => Promise.reject('首刷失败') })
    const { rows, error, refresh } = usePortfolioOverview()
    await refresh()
    expect(error.value).toBe('首刷失败')
    expect(rows.value).toEqual([])

    baseInvoke()
    await refresh()
    expect(error.value).toBeNull()
    expect(rows.value.length).toBe(2)
  })

  it('挂载首刷失败（onMounted 自动首刷）：不再产生未处理 rejection，进入 error 终态并弹 toast', async () => {
    baseInvoke({ list_holdings: () => Promise.reject(new Error('首刷失败')) })
    const sink = makeFakeSink()
    registerToastSink(sink)
    const wrapper = mount(Host)
    await flushPromises()
    expect(wrapper.vm.shell.error.value).toBe('首刷失败')
    expect(wrapper.vm.shell.rows.value).toEqual([])
    expect(sink.error).toHaveBeenCalledWith('首刷失败')
  })
})
