import { computed, onScopeDispose, ref, readonly, type Ref } from 'vue'
import { useReferenceStore } from '@/stores/reference'
import { matchLabel } from '@/utils/pinyin-filter'
import { sumByCurrency, type CurrencyAmountGroup, type PortfolioRow } from '@/composables/usePortfolioOverview'

/**
 * 持仓页签三维过滤排序深模块（issue #902，工厂形态 composable）：
 * 「过滤意图进、可观察状态与派生集合出」——输入持仓行集合（数据拉取归
 * usePortfolioOverview，与首页投资概览卡共享同一拼装接缝），内化搜索匹配、
 * 账户过滤、列头排序三维闭集与过滤 × 合计派生，全部在前端内存完成
 * （`list_holdings` 契约不动）。
 *
 * 与交易过滤（useTransactionFilter）形态学同源但独立实现：数据域不同、
 * 无分页无 URL 下钻无会话保留——三维状态全瞬态，实例随页签挂载而生、
 * 卸载而灭，进入投资视图一律回默认。
 *
 * 维度闭集三：
 * - **搜索**：判定目标为「代码 · 名称」等价文本（与标的搜索同规格，无名称
 *   退化为裸代码），走全仓统一模糊搜索语义规格（词条 AND；原文连续子串 ∨
 *   拼音首字母子序列，大小写不敏感，ADR-0027）；输入 300ms 防抖（标的浏览器
 *   先例）。
 * - **账户过滤**：单选（null = 全部默认态），选项与盈亏页账户下拉同源——
 *   投资账户谓词单点收口在参考 store（type = investment）。
 * - **排序**：市值 / 未实现盈亏两列列头升降序（naive-ui 受控 sorter 形态，
 *   行集合由本模块排序产出）；无排序状态时默认标的代码字母序；缺价行
 *  （金额 null）恒排末尾，与方向无关。
 *
 * 合计口径：随**过滤子集**更新（搜索词、账户过滤生效；排序不影响——排序只是
 * 重排行不是换口径）；按币种分组展示口径不变（sumByCurrency 复用）；缺价行
 * 不计入合计。**多币种混合时金额列排序为账户本位币数值直比**——市值/未实现
 * 盈亏按各账户本位币折算，跨币种比较无精确语义，已接受代价（投资域词汇表
 * Holding 词条注记）。
 */

/** 排序维度闭集：市值 / 未实现盈亏两列（与持仓明细表列 key 一致） */
export type HoldingsSortColumn = 'market_value' | 'unrealized_pnl'
export type HoldingsSortOrder = 'ascend' | 'descend'

/** 排序状态：null = 默认标的代码字母序 */
export interface HoldingsSorter {
  columnKey: HoldingsSortColumn
  order: HoldingsSortOrder
}

/** naive-ui `update:sorter` 的单列 sorter 形态（受控排序回传） */
export interface NaiveUiSorterState {
  columnKey: string | number
  order: 'ascend' | 'descend' | false
}

/** 搜索输入防抖时长（标的浏览器 300ms 先例） */
export const HOLDINGS_SEARCH_DEBOUNCE_MS = 300

// ---------------------------------------------------------------------------
// 纯函数：排序（null 恒排末尾、多币种数值直比、默认代码字母序）
// ---------------------------------------------------------------------------

/** 空值恒排末尾的三向比较（与方向无关）：双侧空值相等，单侧空值殿后，
 * 否则交由比较器。金额排序与默认代码字母序共用同一空值语义。 */
function nullsLast<T>(a: T | null, b: T | null, cmp: (x: T, y: T) => number): number {
  if (a === null && b === null) return 0
  if (a === null) return 1
  if (b === null) return -1
  return cmp(a, b)
}

/** 市值/未实现盈亏两列的比较器：缺价行恒排末尾（与方向无关）；
 * 两侧均有值时**数值直比**（跨币种无精确语义，已接受代价）。 */
export function comparePortfolioRows(a: PortfolioRow, b: PortfolioRow, sorter: HoldingsSorter): number {
  const key = sorter.columnKey === 'market_value' ? 'marketValueCents' : 'unrealizedPnlCents'
  return nullsLast(a[key], b[key], (x, y) => {
    const ascending = x - y
    return sorter.order === 'descend' ? -ascending : ascending
  })
}

/** 排序产出新集合（不修改输入）：null 排序状态 = 默认标的代码字母序
 *（稳定中立，不引入默认按金额的隐性口径），缺代码行（标的字典缺行）恒排末尾。 */
export function sortHoldings(rows: PortfolioRow[], sorter: HoldingsSorter | null): PortfolioRow[] {
  const sorted = [...rows]
  if (sorter === null) {
    sorted.sort((a, b) => nullsLast(a.symbol, b.symbol, (x, y) => x.localeCompare(y)))
    return sorted
  }
  return sorted.sort((a, b) => comparePortfolioRows(a, b, sorter))
}

// ---------------------------------------------------------------------------
// 纯函数：模糊匹配（统一语义，判定目标「代码 · 名称」等价文本）
// ---------------------------------------------------------------------------

/** 持仓行的搜索判定文本：「代码 · 名称」等价文本（与标的搜索同规格）；
 * 无名称退化为裸代码，代码与名称皆缺（标的字典缺行）退化为空文本。 */
export function holdingsSearchLabel(row: Pick<PortfolioRow, 'symbol' | 'instrumentName'>): string {
  const name = row.instrumentName?.trim() || null
  if (!name) return row.symbol ?? ''
  return row.symbol ? `${row.symbol} · ${name}` : name
}

/** 搜索判定：统一模糊搜索语义（词条 AND；原文连续子串 ∨ 拼音首字母子序列，
 * 大小写不敏感，实现复用 pinyin-filter 单点）；空/空白搜索恒命中。 */
export function holdingMatchesSearch(row: PortfolioRow, query: string): boolean {
  const trimmed = query.trim()
  if (!trimmed) return true
  return matchLabel(trimmed, holdingsSearchLabel(row))
}

/** 过滤状态快照（纯函数入参形态）：搜索词（已应用值）+ 账户 id（null = 全部） */
export interface HoldingsFilterState {
  search: string
  accountId: string | null
}

/** 过滤产出新集合：账户过滤与搜索 AND 组合，保序（排序在过滤后单独应用）。 */
export function filterHoldings(rows: PortfolioRow[], state: HoldingsFilterState): PortfolioRow[] {
  let out = rows
  if (state.accountId !== null) {
    out = out.filter((r) => r.accountId === state.accountId)
  }
  if (state.search.trim()) {
    out = out.filter((r) => holdingMatchesSearch(r, state.search))
  }
  return out
}

// ---------------------------------------------------------------------------
// 工厂：过滤意图进、可观察状态与派生集合/合计出
// ---------------------------------------------------------------------------

export interface UseHoldingsFilterReturn {
  /** 搜索框输入回显值（即时，未经防抖） */
  readonly searchInput: Ref<string>
  /** 搜索意图：输入回显立即生效，匹配 300ms 防抖后应用 */
  setSearch(input: string): void
  /** 账户过滤状态（null = 全部默认态） */
  readonly accountId: Ref<string | null>
  /** 账户过滤意图（null 即选「全部」） */
  setAccount(id: string | null): void
  /** 排序状态（null = 默认代码字母序） */
  readonly sorter: Ref<HoldingsSorter | null>
  /** naive-ui `update:sorter` 回传入口：order=false（第三态）即清除排序回默认 */
  setSorter(next: NaiveUiSorterState | NaiveUiSorterState[]): void
  /** 派生行集合：过滤（搜索 × 账户）+ 排序后的展示行 */
  readonly filteredRows: Ref<PortfolioRow[]>
  /** 派生合计（按币种分组）：随过滤子集更新，排序不影响，缺价行不计入 */
  readonly totalMarketValueGroups: Ref<CurrencyAmountGroup[]>
  readonly totalUnrealizedPnlGroups: Ref<CurrencyAmountGroup[]>
  /** 账户下拉选项：与盈亏页账户下拉同源（投资账户谓词单点在参考 store） */
  readonly accountOptions: Ref<{ label: string; value: string }[]>
}

export function useHoldingsFilter(rows: Ref<PortfolioRow[]>): UseHoldingsFilterReturn {
  const reference = useReferenceStore()

  const searchInput = ref('')
  const search = ref('')
  const accountId = ref<string | null>(null)
  const sorter = ref<HoldingsSorter | null>(null)

  let debounceTimer: ReturnType<typeof setTimeout> | undefined

  function setSearch(input: string) {
    searchInput.value = input
    clearTimeout(debounceTimer)
    debounceTimer = setTimeout(() => {
      search.value = input.trim()
    }, HOLDINGS_SEARCH_DEBOUNCE_MS)
  }

  function setAccount(id: string | null) {
    accountId.value = id
  }

  function setSorter(next: NaiveUiSorterState | NaiveUiSorterState[]) {
    // naive-ui 单列排序闭集：数组形态（多列回传）不属本模块维度，视作清除
    const single = Array.isArray(next) ? next[0] : next
    if (
      !single ||
      single.order === false ||
      (single.columnKey !== 'market_value' && single.columnKey !== 'unrealized_pnl')
    ) {
      sorter.value = null
      return
    }
    sorter.value = { columnKey: single.columnKey, order: single.order }
  }

  // 派生链：过滤（搜索 × 账户）→ 排序 → 合计。合计只依赖过滤子集，
  // 排序变化不触碰合计（排序不影响合计由派生结构保证，非调用方自觉）。
  const filteredRows = computed(() =>
    sortHoldings(
      filterHoldings(rows.value, { search: search.value, accountId: accountId.value }),
      sorter.value,
    ),
  )
  const totalMarketValueGroups = computed(() =>
    sumByCurrency(
      filteredRows.value.map((r) => ({ currencyCode: r.valueCurrencyCode, cents: r.marketValueCents })),
    ),
  )
  const totalUnrealizedPnlGroups = computed(() =>
    sumByCurrency(
      filteredRows.value.map((r) => ({
        currencyCode: r.valueCurrencyCode,
        cents: r.unrealizedPnlCents,
      })),
    ),
  )

  const accountOptions = computed(() =>
    reference.investmentAccounts.map((a) => ({ label: a.name, value: a.id })),
  )

  // 页签卸载即实例消亡：防抖定时器随作用域清理，不向卸载后的行集合写回
  onScopeDispose(() => {
    clearTimeout(debounceTimer)
  })

  return {
    searchInput: readonly(searchInput),
    setSearch,
    accountId: readonly(accountId),
    setAccount,
    sorter: readonly(sorter),
    setSorter,
    filteredRows,
    totalMarketValueGroups,
    totalUnrealizedPnlGroups,
    accountOptions,
  }
}
