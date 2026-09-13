import { computed, onScopeDispose, readonly, type Ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useReferenceStore } from '@/stores/reference'
import { matchLabel } from '@/utils/pinyin-filter'
import { sumByCurrency, type CurrencyAmountGroup, type PortfolioRow } from '@/composables/usePortfolioOverview'
import {
  HOLDINGS_PAGE_SIZE,
  useInvestmentsSessionStore,
  type HoldingsSorter,
  type NaiveUiSorterState,
} from '@/stores/investments-session'

/**
 * 持仓页签三维过滤排序深模块（issue #902，工厂形态 composable；状态归宿见下）：
 * 「过滤意图进、可观察状态与派生集合出」——输入持仓行集合（数据拉取归
 * usePortfolioOverview，与首页投资概览卡共享同一拼装接缝），内化搜索匹配、
 * 账户过滤、列头排序三维闭集与过滤 × 合计派生，全部在前端内存完成
 * （`list_holdings` 契约不动）。
 *
 * 与交易过滤（useTransactionFilter）形态学同源但独立实现：数据域不同、
 * 无 URL 下钻。**issue #1192 起三维状态与页码住投资页会话状态 store**
 * （ADR-0094 会话内保留，spec #898/#902 的瞬态豁免随本票落地）：本工厂退为
 * 薄适配——状态投影与意图入口原样转交会话 store，派生链仍在调用方实例内，
 * 切片仍由表格组件内置分页完成。卸载重挂恢复离开时的选择、冷启动回默认、
 * 全程零写盘；ESC 复位由投资视图向复位回调注册表声明（见 store resetToDefault）。
 * 作用域销毁即撤销在途搜索防抖（Spec 轴 finding）：已输入未应用的值不落地，
 * 回显回到最后应用值——与原实例级实现的 onScopeDispose 行为等价。
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
 *
 * 分页（issue #912）：过滤排序之后派生行集的**展示切片**，不是第四个过滤
 * 维度——本模块只持页码状态与「翻页归零」语义（三维任一应用值实际变化即回
 * 第一页；页大小固定 20 不设选择器），切片由表格组件内置分页完成；合计与
 * 空态在切片前判定，与可见页无关。
 */

/** 排序维度闭集与常量随状态迁入投资页会话 store（issue #1192）；此处再导出维持
 * 既有导入路径（消费方经本模块取用），不制造第二口径。 */
export {
  HOLDINGS_SEARCH_DEBOUNCE_MS,
  HOLDINGS_PAGE_SIZE,
} from '@/stores/investments-session'
export type {
  HoldingsSortColumn,
  HoldingsSortOrder,
  HoldingsSorter,
  NaiveUiSorterState,
} from '@/stores/investments-session'

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
  /** 页码（1 起）：过滤排序之后派生行集的展示切片，不是第四个过滤维度 */
  readonly page: Ref<number>
  /** 派生合计（按币种分组）：随过滤子集更新，排序不影响，缺价行不计入 */
  readonly totalMarketValueGroups: Ref<CurrencyAmountGroup[]>
  readonly totalUnrealizedPnlGroups: Ref<CurrencyAmountGroup[]>
  /** 账户下拉选项：与盈亏页账户下拉同源（投资账户谓词单点在参考 store） */
  readonly accountOptions: Ref<{ label: string; value: string }[]>
}

export function useHoldingsFilter(rows: Ref<PortfolioRow[]>): UseHoldingsFilterReturn {
  const reference = useReferenceStore()
  const session = useInvestmentsSessionStore()
  // 会话级 store 是三维状态与页码的唯一读写方（ADR-0094）；本工厂只做投影，
  // 投影一律只读（原实现的 readonly(...) 保证不因状态迁入 store 而消失）。
  const { holdingsSearchInput, holdingsAccountId: accountId, holdingsSorter: sorter, holdingsPage: storedPage } =
    storeToRefs(session)
  const searchInput = readonly(holdingsSearchInput)

  // 派生链：过滤（搜索 × 账户）→ 排序 → 合计。合计只依赖过滤子集，
  // 排序变化不触碰合计（排序不影响合计由派生结构保证，非调用方自觉）。
  const filteredRows = computed(() =>
    sortHoldings(
      filterHoldings(rows.value, { search: session.holdingsSearch, accountId: accountId.value }),
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

  // 页码恢复钳制（ADR-0094 决策 3 / ADR-0045「回退不归零」既有出口的等价形态）：
  // 恢复/回访时行集可能少于离开时的页码（离开期间清仓或过滤收窄）——落到有效
  // 范围并写回 store，使保留态与展示一致；只钳展示会让陈旧页码在行集回增后
  // 重新生效、凭空跳页。持仓是内存切片（无超页请求、无服务端空页信号），故
  // 钳制点在派生行集而非请求响应，语义仍是「回退到有效范围、不新增第二出口」。
  // 读出口即对账（不另挂 watcher/effect）：任一消费方读到越界页码时回写一次，
  // 使内存保留态与展示同源；翻页意图与视图 onChange 直接写本 ref。
  let lastReconciled: { stored: number; valid: number } | null = null
  const page = computed<number>({
    get: () => {
      const valid = pageCount(filteredRows.value)
      const stored = storedPage.value
      const clamped = Math.min(stored, valid)
      if (
        clamped !== stored &&
        (lastReconciled === null ||
          lastReconciled.stored !== stored ||
          lastReconciled.valid !== valid)
      ) {
        lastReconciled = { stored, valid }
        // 回写把保留态收进有效范围（只会往小走），翻页归零 watch 不受影响
        session.setPage(clamped)
      }
      return clamped
    },
    set: (next) => {
      lastReconciled = null
      session.setPage(next)
    },
  })

  // 页签卸载（含导航离开投资视图）即撤销在途搜索防抖：已输入未应用的值不落地
  onScopeDispose(() => {
    session.cancelPendingSearch()
  })

  return {
    searchInput,
    setSearch: session.setSearch,
    accountId,
    setAccount: session.setAccount,
    sorter,
    setSorter: session.setSorter,
    filteredRows,
    page,
    totalMarketValueGroups,
    totalUnrealizedPnlGroups,
    accountOptions,
  }
}

/** 派生行集的有效页数（至少 1 页：空集与单页同归第 1 页）。 */
function pageCount(rows: PortfolioRow[]): number {
  return Math.max(1, Math.ceil(rows.length / HOLDINGS_PAGE_SIZE))
}
