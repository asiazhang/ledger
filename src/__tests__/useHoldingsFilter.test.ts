import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { ref, effectScope } from 'vue'
import { flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { mockInvoke, wireInvokeSeam } from './helpers/invoke-mock'
import { REFERENCE_DEFAULTS } from './helpers/reference-stubs'
import { useReferenceStore } from '@/stores/reference'
import {
  HOLDINGS_SEARCH_DEBOUNCE_MS,
  filterHoldings,
  holdingsSearchLabel,
  holdingMatchesSearch,
  sortHoldings,
  useHoldingsFilter,
} from '@/composables/useHoldingsFilter'
import type { PortfolioRow } from '@/composables/usePortfolioOverview'
import type { Account } from '@/types'

// ---------------------------------------------------------------------------
// 夹具：四行持仓，覆盖两账户 / 三币种 / 缺价行 / 中英文名称
// ---------------------------------------------------------------------------

function makeRow(partial: Partial<PortfolioRow> & { holdingId: string }): PortfolioRow {
  return {
    accountId: 'acc-inv-1',
    accountName: 'A股账户',
    instrumentId: `inst-${partial.holdingId}`,
    symbol: null,
    instrumentName: null,
    quantity: 100,
    costBasisCents: 100000,
    costCurrencyCode: 'CNY',
    latestPriceCents: null,
    latestPriceCurrencyCode: null,
    latestNavDate: null,
    marketValueCents: null,
    unrealizedPnlCents: null,
    valueCurrencyCode: 'CNY',
    ...partial,
  }
}

/** h2 无行情（全 null 金额）——缺价行语义的常驻样本 */
const FIXTURE_ROWS: PortfolioRow[] = [
  makeRow({
    holdingId: 'h1',
    symbol: '600000',
    instrumentName: '浦发银行',
    marketValueCents: 150000,
    unrealizedPnlCents: 30000,
    valueCurrencyCode: 'CNY',
  }),
  makeRow({ holdingId: 'h2', symbol: '000001', instrumentName: '平安银行' }),
  makeRow({
    holdingId: 'h3',
    accountId: 'acc-inv-2',
    accountName: '港美账户',
    symbol: '00700',
    instrumentName: '腾讯控股',
    marketValueCents: 2000000,
    unrealizedPnlCents: -50000,
    valueCurrencyCode: 'HKD',
  }),
  makeRow({
    holdingId: 'h4',
    accountId: 'acc-inv-2',
    accountName: '港美账户',
    symbol: 'AAPL',
    instrumentName: 'Apple',
    marketValueCents: 500000,
    unrealizedPnlCents: 10000,
    valueCurrencyCode: 'USD',
  }),
]

const ids = (rows: PortfolioRow[]) => rows.map((r) => r.holdingId)

// ---------------------------------------------------------------------------
// 纯函数：排序语义（null 恒排末尾、多币种数值直比、默认代码字母序）
// ---------------------------------------------------------------------------

describe('sortHoldings 排序语义', () => {
  it('无排序状态时按标的代码字母序，缺代码行恒排末尾', () => {
    const rows = [
      makeRow({ holdingId: 'a', symbol: '600000' }),
      makeRow({ holdingId: 'b', symbol: null }),
      makeRow({ holdingId: 'c', symbol: '00700' }),
      makeRow({ holdingId: 'd', symbol: null }),
    ]
    expect(ids(sortHoldings(rows, null))).toEqual(['c', 'a', 'b', 'd'])
  })

  it('市值升序：数值递增，缺价行恒排末尾', () => {
    expect(ids(sortHoldings(FIXTURE_ROWS, { columnKey: 'market_value', order: 'ascend' }))).toEqual([
      'h1',
      'h4',
      'h3',
      'h2',
    ])
  })

  it('市值降序：数值递减，缺价行仍恒排末尾（与方向无关）', () => {
    expect(ids(sortHoldings(FIXTURE_ROWS, { columnKey: 'market_value', order: 'descend' }))).toEqual([
      'h3',
      'h4',
      'h1',
      'h2',
    ])
  })

  it('未实现盈亏降序：盈利在前亏损在后，缺价行恒排末尾', () => {
    expect(ids(sortHoldings(FIXTURE_ROWS, { columnKey: 'unrealized_pnl', order: 'descend' }))).toEqual([
      'h1',
      'h4',
      'h3',
      'h2',
    ])
  })

  it('多币种混合时金额列为数值直比（账户本位币不做汇率折算，已接受代价）', () => {
    // h1 = 150000 分(CNY)、h4 = 500000 分(USD)：直比分值 150000 < 500000，不做任何折算
    const rows = [FIXTURE_ROWS[0]!, FIXTURE_ROWS[3]!]
    expect(ids(sortHoldings(rows, { columnKey: 'market_value', order: 'ascend' }))).toEqual(['h1', 'h4'])
    expect(ids(sortHoldings(rows, { columnKey: 'market_value', order: 'descend' }))).toEqual(['h4', 'h1'])
  })

  it('不修改输入数组（排序产出新集合）', () => {
    const rows = [...FIXTURE_ROWS]
    sortHoldings(rows, { columnKey: 'market_value', order: 'descend' })
    expect(ids(rows)).toEqual(['h1', 'h2', 'h3', 'h4'])
  })
})

// ---------------------------------------------------------------------------
// 纯函数：模糊匹配语义（统一模糊搜索语义规格，判定目标「代码 · 名称」等价文本）
// ---------------------------------------------------------------------------

describe('holdingsSearchLabel / holdingMatchesSearch 匹配语义', () => {
  it('判定目标为「代码 · 名称」等价文本，无名称退化为裸代码，两者皆缺为空文本', () => {
    expect(holdingsSearchLabel({ symbol: '600000', instrumentName: '浦发银行' })).toBe('600000 · 浦发银行')
    expect(holdingsSearchLabel({ symbol: '600000', instrumentName: null })).toBe('600000')
    expect(holdingsSearchLabel({ symbol: null, instrumentName: null })).toBe('')
  })

  it('按代码原文子串命中（大小写不敏感）', () => {
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, '600')).toBe(true)
    expect(holdingMatchesSearch(FIXTURE_ROWS[3]!, 'aapl')).toBe(true)
  })

  it('按名称原文子串命中', () => {
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, '浦发')).toBe(true)
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, '浦发银行')).toBe(true)
  })

  it('按拼音首字母子序列命中', () => {
    // 浦发银行 → pfyh；腾讯控股 → txkg
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, 'pfyh')).toBe(true)
    expect(holdingMatchesSearch(FIXTURE_ROWS[2]!, 'txkg')).toBe(true)
  })

  it('多词条之间 AND，组合命中代码与名称', () => {
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, '600000 浦发')).toBe(true)
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, '600000 腾讯')).toBe(false)
  })

  it('空/空白搜索恒命中（恢复完整列表）', () => {
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, '')).toBe(true)
    expect(holdingMatchesSearch(FIXTURE_ROWS[0]!, '   ')).toBe(true)
  })
})

describe('filterHoldings 过滤语义', () => {
  it('账户过滤：仅保留所选账户行', () => {
    expect(ids(filterHoldings(FIXTURE_ROWS, { search: '', accountId: 'acc-inv-2' }))).toEqual(['h3', 'h4'])
  })

  it('搜索与账户过滤 AND 组合', () => {
    expect(
      ids(filterHoldings(FIXTURE_ROWS, { search: 'txkg', accountId: 'acc-inv-2' })),
    ).toEqual(['h3'])
    expect(
      ids(filterHoldings(FIXTURE_ROWS, { search: 'txkg', accountId: 'acc-inv-1' })),
    ).toEqual([])
  })

  it('全部默认态（account = null、空搜索）返回全量', () => {
    expect(ids(filterHoldings(FIXTURE_ROWS, { search: '', accountId: null }))).toHaveLength(4)
  })
})

// ---------------------------------------------------------------------------
// 工厂形态 composable：过滤意图进、可观察状态与派生合计出（零 IPC 触达）
// ---------------------------------------------------------------------------

/** 账户下拉同源夹具：投资账户谓词收口（cash / 已删非投资账户不进选项面） */
const FILTER_ACCOUNTS: Account[] = [
  {
    id: 'acc-cash',
    name: '现金',
    type: 'cash',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
  {
    id: 'acc-inv-1',
    name: 'A股账户',
    type: 'investment',
    currency_code: 'CNY',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
  {
    id: 'acc-inv-2',
    name: '港美账户',
    type: 'investment',
    currency_code: 'HKD',
    initial_balance_cents: 0,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    version: 1,
    device_id: 'test',
    is_deleted: false,
    is_hidden: false,
  },
]

describe('useHoldingsFilter 工厂', () => {
  let scope: ReturnType<typeof effectScope> | undefined

  function setup(rows: PortfolioRow[]) {
    const rowsRef = ref(rows)
    let instance: ReturnType<typeof useHoldingsFilter> | undefined
    scope = effectScope()
    scope.run(() => {
      instance = useHoldingsFilter(rowsRef)
    })
    return instance!
  }

  beforeEach(async () => {
    vi.useFakeTimers()
    setActivePinia(createPinia())
    wireInvokeSeam({ overrides: { list_accounts: FILTER_ACCOUNTS } })
    await useReferenceStore().refresh()
  })

  afterEach(() => {
    vi.useRealTimers()
    scope?.stop()
  })

  it('默认态：全量行按代码字母序，合计为全量口径（按币种分组、缺价行不计入）', () => {
    const hf = setup(FIXTURE_ROWS)
    expect(ids(hf.filteredRows.value)).toEqual(['h2', 'h3', 'h1', 'h4'])
    // CNY = h1 的 150000（h2 缺价跳过）、HKD = 2000000、USD = 500000
    expect(hf.totalMarketValueGroups.value).toEqual([
      { currencyCode: 'CNY', cents: 150000 },
      { currencyCode: 'HKD', cents: 2000000 },
      { currencyCode: 'USD', cents: 500000 },
    ])
    expect(hf.totalUnrealizedPnlGroups.value).toEqual([
      { currencyCode: 'CNY', cents: 30000 },
      { currencyCode: 'HKD', cents: -50000 },
      { currencyCode: 'USD', cents: 10000 },
    ])
  })

  it('搜索意图 300ms 防抖生效：输入即时回显、行集合延迟收窄，清空后恢复完整列表', async () => {
    const hf = setup(FIXTURE_ROWS)
    hf.setSearch('txkg')
    expect(hf.searchInput.value).toBe('txkg')
    // 防抖窗口内行集合未变
    expect(ids(hf.filteredRows.value)).toHaveLength(4)
    vi.advanceTimersByTime(HOLDINGS_SEARCH_DEBOUNCE_MS)
    await flushPromises()
    expect(ids(hf.filteredRows.value)).toEqual(['h3'])

    hf.setSearch('')
    vi.advanceTimersByTime(HOLDINGS_SEARCH_DEBOUNCE_MS)
    await flushPromises()
    expect(ids(hf.filteredRows.value)).toHaveLength(4)
  })

  it('账户过滤意图立即生效，选「全部」（null）恢复完整列表', () => {
    const hf = setup(FIXTURE_ROWS)
    hf.setAccount('acc-inv-2')
    expect(ids(hf.filteredRows.value)).toEqual(['h3', 'h4'])
    hf.setAccount(null)
    expect(ids(hf.filteredRows.value)).toHaveLength(4)
  })

  it('合计随过滤子集更新（搜索 × 账户），排序不影响合计', async () => {
    const hf = setup(FIXTURE_ROWS)
    hf.setAccount('acc-inv-2')
    hf.setSearch('txkg')
    vi.advanceTimersByTime(HOLDINGS_SEARCH_DEBOUNCE_MS)
    await flushPromises()
    expect(hf.totalMarketValueGroups.value).toEqual([{ currencyCode: 'HKD', cents: 2000000 }])

    // 排序只是重排行，不是换口径：合计不动
    hf.setSorter({ columnKey: 'market_value', order: 'descend' })
    expect(ids(hf.filteredRows.value)).toEqual(['h3'])
    expect(hf.totalMarketValueGroups.value).toEqual([{ currencyCode: 'HKD', cents: 2000000 }])
  })

  it('排序意图：naive-ui 单列 sorter 形态接入，order=false / 第三次点击回默认代码序', () => {
    const hf = setup(FIXTURE_ROWS)
    hf.setSorter({ columnKey: 'market_value', order: 'ascend' })
    expect(ids(hf.filteredRows.value)).toEqual(['h1', 'h4', 'h3', 'h2'])
    // 再点同列切降序；order=false（第三态）清除排序回默认
    hf.setSorter({ columnKey: 'market_value', order: false })
    expect(ids(hf.filteredRows.value)).toEqual(['h2', 'h3', 'h1', 'h4'])
  })

  it('账户下拉选项与盈亏页同源：投资账户谓词收口（cash 不进选项面）', () => {
    const hf = setup(FIXTURE_ROWS)
    expect(hf.accountOptions.value).toEqual([
      { label: 'A股账户', value: 'acc-inv-1' },
      { label: '港美账户', value: 'acc-inv-2' },
    ])
  })

  it('零 IPC 触达：过滤全在前端内存完成，行数据归调用方供给', async () => {
    setup(FIXTURE_ROWS)
    await flushPromises()
    // 参考字典五命令是参考 store 自举（账户下拉选项面），其余命令一律不触达——
    // 尤其 list_holdings / list_instruments 不经本模块
    const nonReferenceCmds = mockInvoke.mock.calls
      .map(([cmd]) => cmd)
      .filter((cmd) => !(cmd in REFERENCE_DEFAULTS))
    expect(nonReferenceCmds).toEqual([])
  })
})
