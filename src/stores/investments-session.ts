import { defineStore } from 'pinia'
import { computed, readonly, ref, watch } from 'vue'
import type { Instrument } from '@ledger/types'

/** 持仓页签排序列闭集（与持仓明细表列 key 一致）：市值 / 持仓收益（未实现盈亏） */
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

/** 分页页大小：固定值不设选择器（全仓先例：交易页与搜索页同为 20，issue #912） */
export const HOLDINGS_PAGE_SIZE = 20

/** 投资页默认页签（冷启动与 ESC 复位共用同一默认态来源）。 */
export const INVESTMENTS_DEFAULT_TAB = 'pnl'

/** 走势视图模式：组合市值曲线 ↔ 单标的曲线同视图切换 */
export type TrendViewMode = 'portfolio' | 'instrument'

/** 走势预设区间闭集：1 月 / 3 月 / 1 年 / 全部（ADR-0019） */
export type TrendRangePreset = '1m' | '3m' | '1y' | 'all'

/** 走势默认预设区间：近一年（两年回填的中间视角，其余区间一键切换） */
export const TREND_PRESET_DEFAULT: TrendRangePreset = '1y'

/** 走势默认视图模式：组合市值曲线 */
export const TREND_MODE_DEFAULT: TrendViewMode = 'portfolio'

/**
 * 投资页会话状态 store（issue #1192）：投资页四页签瞬态选择的唯一读写方——
 * 当前页签 + 持仓页签筛选三维（搜索/账户过滤/排序）与页码 + 走势页签选中标的
 * （模式/预设区间/单标的）提升到会话生命周期（ADR-0094，本票前唯一残留的
 * 「会话内保留」显式豁免，spec #898/#902 的 Out of Scope 随本票落地）。
 *
 * 「会话内保留」语义（ADR-0094，先例 reports-session #427 / 交易页 #893）：
 * 同一应用会话内，切走页签再切回（或经侧栏离开投资视图再回来）回到离开时的
 * 样子，数据照常按恢复的选择现拉（持仓全量拉取后前端过滤排序，无第二份查询
 * 口径）；应用冷启动（新 pinia）回默认（默认页签 + 全量持仓 + 组合走势）。
 * 保留全程零写盘（不进 localStorage / SQLite）、不写回 URL。
 *
 * 「意图进、状态出」的小深模块，规则内化其中（视图不再各持一份瞬态）：
 * - 搜索 300ms 防抖内化为唯一写路径（输入回显即时、应用值延迟），防抖期间离开
 *   页签不留下迟到写入——`resetToDefault` 先撤销在途定时器再清应用值，复位后
 *   不会有意外的旧输入落地；
 * - 翻页归零：筛选三维任一「应用值」实际变化即回第一页（watch 只对实际变化
 *   响应，同值重设不归零）；页码是展示切片不是第四维；
 * - ESC 复位出口（ADR-0094）：resetToDefault 全维回默认，视图经复位回调注册表
 *   （viewResetRegistry）向窗口行为守卫声明本出口。
 */
export const useInvestmentsSessionStore = defineStore('investments-session', () => {
  /** 当前页签（会话内保留、冷启动回默认「盈亏」；原为视图内实例级瞬态）。 */
  const activeTab = ref<string>(INVESTMENTS_DEFAULT_TAB)

  /** 持仓搜索输入回显值（即时，未经防抖） */
  const holdingsSearchInput = ref('')
  /** 持仓搜索应用值（防抖后参与过滤） */
  const holdingsSearch = ref('')
  /** 持仓账户过滤（null = 全部默认态） */
  const holdingsAccountId = ref<string | null>(null)
  /** 持仓排序（null = 默认标的代码字母序） */
  const holdingsSorter = ref<HoldingsSorter | null>(null)
  /** 持仓页码（1 起）：过滤排序之后派生行集的展示切片 */
  const holdingsPage = ref(1)

  /** 走势视图模式与预设区间（会话内保留、冷启动回默认） */
  const trendMode = ref<TrendViewMode>(TREND_MODE_DEFAULT)
  const trendPreset = ref<TrendRangePreset>(TREND_PRESET_DEFAULT)
  /** 走势页签选中标的（null = 未选，会话内保留、冷启动回默认） */
  const trendInstrumentId = ref<string | null>(null)

  /** 最近一次选中的标的投影：id → 标的本体（标的不在标的字典分页内时仍可供
   * 走势面板取数出图，与入口形态一致）。 */
  const trendInstrumentCache = new Map<string, Instrument>()

  /** 搜索防抖定时器：闭包内单个，store 实例唯一（视图卸载不撤销定时器——
   * 落地与卸载解耦，防抖语义随会话状态常驻）。 */
  let searchTimer: ReturnType<typeof setTimeout> | undefined

  function setSearch(input: string) {
    holdingsSearchInput.value = input
    clearTimeout(searchTimer)
    const next = input.trim()
    // 同值不重排定时器：重复输入同一有效值时应用值本就不变
    if (next === holdingsSearch.value) return
    searchTimer = setTimeout(() => {
      holdingsSearch.value = next
    }, HOLDINGS_SEARCH_DEBOUNCE_MS)
  }

  function setAccount(id: string | null) {
    holdingsAccountId.value = id
  }

  /** naive-ui `update:sorter` 回传入口：order=false（第三态）或非本表列即清除排序回默认 */
  function setSorter(next: NaiveUiSorterState | NaiveUiSorterState[]) {
    // naive-ui 单列排序闭集：数组形态（多列回传）不属本模块维度，视作清除
    const single = Array.isArray(next) ? next[0] : next
    let resolved: HoldingsSorter | null = null
    if (
      single &&
      single.order !== false &&
      (single.columnKey === 'market_value' || single.columnKey === 'unrealized_pnl')
    ) {
      resolved = { columnKey: single.columnKey, order: single.order }
    }
    // 同值重设不产生新状态（翻页归零只对实际变化响应）
    if (
      holdingsSorter.value?.columnKey === resolved?.columnKey &&
      holdingsSorter.value?.order === resolved?.order
    ) {
      return
    }
    holdingsSorter.value = resolved
  }

  function setPage(next: number) {
    holdingsPage.value = next
  }

  /** 翻页归零：三维任一应用值实际变化即回第一页（排序清除亦属实际变化）。
   * 同步 flush 使归零与意图应用原子生效，不留「维度已变、页码未归」的中间态；
   * 防抖中的搜索不归零（输入回显不是应用值）。 */
  watch([holdingsSearch, holdingsAccountId, holdingsSorter], () => {
    holdingsPage.value = 1
  }, { flush: 'sync' })

  /** 页签写入意图入口（NTabs v-model 经视图桥接；测试经本入口换档） */
  function setActiveTab(tab: string) {
    activeTab.value = tab
  }

  /** 走势入口（标的列表「走势」按钮与 focus 落点共用）：带入标的并切到走势页签 */
  function showTrendInstrument(instrument: Instrument) {
    trendInstrumentCache.set(instrument.id, instrument)
    trendInstrumentId.value = instrument.id
    trendMode.value = 'instrument'
  }

  /** 走势面板标的选中意图（id 进；null = 清除选中回组合模式） */
  function selectTrendInstrument(id: string | null) {
    if (id === null) {
      trendInstrumentId.value = null
      trendMode.value = TREND_MODE_DEFAULT
      return
    }
    if (!trendInstrumentCache.has(id)) return
    trendInstrumentId.value = id
    trendMode.value = 'instrument'
  }

  /** 走势模式写入意图（面板 NRadioGroup v-model 回传） */
  function setTrendMode(mode: TrendViewMode) {
    trendMode.value = mode
  }

  /** 走势预设区间写入意图（面板 NRadioGroup v-model 回传） */
  function setTrendPreset(preset: TrendRangePreset) {
    trendPreset.value = preset
  }

  /** 当前选中标的投影（未选中的 id 回 null；投影随会话保留） */
  const trendInstrument = computed<Instrument | null>(() =>
    trendInstrumentId.value === null
      ? null
      : trendInstrumentCache.get(trendInstrumentId.value) ?? null,
  )

  /**
   * ESC 复位出口（ADR-0094 决策 4）：页签回默认「盈亏」、持仓筛选三维清零、
   * 翻页归零、走势回默认组合曲线（选中标的与单标的模式一并清除）——复位即清除
   * 保留态本身（复位后离开再回来 = 默认）。走各维既有写入出口，同值幂等无操作；
   * 在途搜索防抖先撤销，复位后不会有意外的旧输入落地。
   */
  function resetToDefault() {
    clearTimeout(searchTimer)
    activeTab.value = INVESTMENTS_DEFAULT_TAB
    holdingsSearchInput.value = ''
    holdingsSearch.value = ''
    holdingsAccountId.value = null
    holdingsSorter.value = null
    holdingsPage.value = 1
    trendInstrumentId.value = null
    trendInstrumentCache.clear()
    trendMode.value = TREND_MODE_DEFAULT
    trendPreset.value = TREND_PRESET_DEFAULT
  }

  return {
    // 可写状态仅限受控组件直写语义：页签（NTabs v-model 桥接）、搜索输入回显
    // （NInput v-model 语义）。两者都是「意图即值」的受控回传，无中间规则；
    // 其余维度只读、改动只经意图入口（含走势模式与预设区间——面板经组合式函数
    // 的写入口转交，语义在 store 内单点）。
    activeTab,
    holdingsSearchInput,
    // 只读投影（改动只经意图入口）
    holdingsSearch: readonly(holdingsSearch),
    holdingsAccountId: readonly(holdingsAccountId),
    holdingsSorter: readonly(holdingsSorter),
    holdingsPage: readonly(holdingsPage),
    trendMode: readonly(trendMode),
    trendPreset: readonly(trendPreset),
    trendInstrumentId: readonly(trendInstrumentId),
    trendInstrument,
    // 意图入口
    setActiveTab,
    setSearch,
    setAccount,
    setSorter,
    setPage,
    showTrendInstrument,
    selectTrendInstrument,
    setTrendMode,
    setTrendPreset,
    resetToDefault,
  }
})
