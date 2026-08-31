import { computed, onMounted, ref, watch } from 'vue'
import { api } from '@/api'
import { usePricesChanged } from '@/composables/usePricesChanged'
import type {
  Instrument,
  InstrumentType,
  InstrumentPriceTrend,
  PortfolioValueTrend,
  TrendRange,
} from '@/types'

/** 走势预设区间：1 月 / 3 月 / 1 年 / 全部（ADR-0019） */
export type TrendRangePreset = '1m' | '3m' | '1y' | 'all'

export const TREND_RANGE_PRESETS: { value: TrendRangePreset; labelKey: string }[] = [
  { value: '1m', labelKey: 'investments.trend.range1m' },
  { value: '3m', labelKey: 'investments.trend.range3m' },
  { value: '1y', labelKey: 'investments.trend.range1y' },
  { value: 'all', labelKey: 'investments.trend.rangeAll' },
]

/** 走势视图模式：组合市值曲线 ↔ 单标的曲线同视图切换 */
export type TrendMode = 'portfolio' | 'instrument'

/** 有行情来源的标的类型（东财日 K 数据源覆盖范围，与 PriceHistory 口径一致） */
const MARKET_SOURCE_TYPES: readonly InstrumentType[] = ['stock', 'etf']

/** 某标的是否走行情采集通道：股票 / ETF 且市场已知（未知市场无法构造 secid） */
export function hasMarketSource(inst: Pick<Instrument, 'type' | 'market'>): boolean {
  return MARKET_SOURCE_TYPES.includes(inst.type) && inst.market !== 'unknown'
}

/** 某月天数（month 1-12） */
function daysInMonth(year: number, month: number): number {
  return new Date(year, month, 0).getDate()
}

function pad2(n: number): string {
  return String(n).padStart(2, '0')
}

/**
 * 预设区间 → 查询区间（纯函数）：起点 = today 减去对应时长（月末溢出钳制到
 * 目标月最后一日），终点不设界（后端裁剪到今天为止）。「全部」不设起止。
 */
export function toTrendRange(preset: TrendRangePreset, today: Date): TrendRange {
  if (preset === 'all') return {}
  const months = preset === '1m' ? 1 : preset === '3m' ? 3 : 12
  const total = today.getFullYear() * 12 + today.getMonth() - months
  const year = Math.floor(total / 12)
  const month = (total % 12) + 1
  const day = Math.min(today.getDate(), daysInMonth(year, month))
  return { start_date: `${year}-${pad2(month)}-${pad2(day)}`, end_date: null }
}

/** 图表序列：x 轴按周连续的日期槽位 + y 轴数值（分；缺周为 null，由 spanGaps 跨越） */
export interface TrendChartSeries {
  labels: string[]
  values: (number | null)[]
}

/** 取 ISO 日期（YYYY-MM-DD）所在周的周一（本地正午构造，避开 DST 漂移） */
function mondayOf(isoDate: string): Date {
  const [year, month, day] = isoDate.split('-').map(Number)
  const date = new Date(year, month - 1, day, 12)
  date.setDate(date.getDate() - ((date.getDay() + 6) % 7))
  return date
}

function isoOf(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`
}

/**
 * 采样点序列 → 图表数据（纯函数）：x 轴按日期连续——从首点到末点逐周生成槽位，
 * 采样点按所在周归位（label 用真实采样日），缺周填 null 由图表 spanGaps 连点跨越。
 */
export function toTrendChartSeries(points: { date: string; value: number }[]): TrendChartSeries {
  if (points.length === 0) return { labels: [], values: [] }

  const slots: string[] = []
  const cursor = mondayOf(points[0].date)
  const end = mondayOf(points[points.length - 1].date)
  while (cursor <= end) {
    slots.push(isoOf(cursor))
    cursor.setDate(cursor.getDate() + 7)
  }

  const pointByWeek = new Map(points.map((p) => [mondayOf(p.date).getTime(), p]))
  return {
    labels: slots.map((slot) => pointByWeek.get(mondayOf(slot).getTime())?.date ?? slot),
    values: slots.map((slot) => pointByWeek.get(mondayOf(slot).getTime())?.value ?? null),
  }
}

/** 空态判定：无任何采样点即无历史数据 */
export function isTrendEmpty(points: { date: string; value: number }[]): boolean {
  return points.length === 0
}

/** 默认预设区间：近一年（两年回填的中间视角，其余区间一键切换） */
const DEFAULT_PRESET: TrendRangePreset = '1y'

/** 单标的字典每页条数上限（list_instruments 单页上限，用于走势面板标的下拉） */
const INSTRUMENT_FETCH_LIMIT = 500

/**
 * 投资资产走势数据层（issue #139 / ADR-0019）：收口组合 / 单标的两种走势的
 * 获取与转换。查询区间由预设区间派生；数据由 T3 命令给出（区间裁剪、首有效
 * 点起始在后端完成），此处只做点位映射与空态判定。
 */
export function usePortfolioTrend() {
  const preset = ref<TrendRangePreset>(DEFAULT_PRESET)
  const mode = ref<TrendMode>('portfolio')
  /** 单标的模式下当前标的；null 表示尚未选择 */
  const instrument = ref<Instrument | null>(null)

  const loading = ref(false)
  const portfolioTrend = ref<PortfolioValueTrend | null>(null)
  const instrumentTrend = ref<InstrumentPriceTrend | null>(null)
  /** 走势面板标的下拉的标的字典（一次拉全） */
  const instruments = ref<Instrument[]>([])

  const range = computed(() => toTrendRange(preset.value, new Date()))

  /** 上次已取数的请求键（模式 + 标的 + 区间起始）：同一键不重复请求，收敛双触发通道 */
  let lastFetchedKey: string | null = null

  async function fetchTrend() {
    if (mode.value === 'instrument') {
      // 未选标的，或非股票/ETF 等无行情来源标的（ADR-0019）：不发起查询，由面板给边界说明
      if (!instrument.value || !hasMarketSource(instrument.value)) return
    }
    const key =
      mode.value === 'portfolio'
        ? `portfolio|${range.value.start_date ?? 'all'}`
        : `instrument|${instrument.value!.id}|${range.value.start_date ?? 'all'}`
    if (key === lastFetchedKey) return
    lastFetchedKey = key
    loading.value = true
    try {
      if (mode.value === 'portfolio') {
        portfolioTrend.value = await api.portfolioValueTrend(range.value)
      } else {
        instrumentTrend.value = await api.instrumentPriceTrend(
          instrument.value!.id,
          range.value,
        )
      }
    } catch (e) {
      // 失败允许同键重试
      lastFetchedKey = null
      throw e
    } finally {
      loading.value = false
    }
  }

  async function fetchInstruments() {
    const res = await api.listInstruments({ page_size: INSTRUMENT_FETCH_LIMIT })
    instruments.value = res.items
  }

  /** 刷新走势数据（预设区间 / 模式 / 标的变化后由 watch 自动触发） */
  async function refresh() {
    await Promise.all([fetchTrend(), instruments.value.length === 0 ? fetchInstruments() : null])
  }

  /** 强制重拉：重置同键去重短路后刷新（价格失效信号：键未变但数据已变） */
  async function forceRefresh() {
    lastFetchedKey = null
    await refresh()
  }

  /** 切到单标的曲线（标的列表「走势」入口与面板下拉共用） */
  function showInstrument(inst: Instrument) {
    mode.value = 'instrument'
    instrument.value = inst
  }

  /** 切回组合市值曲线 */
  function showPortfolio() {
    mode.value = 'portfolio'
  }

  watch([preset, mode, () => instrument.value?.id], () => {
    void refresh()
  })

  // 价格失效信号（ADR-0031）：同步实际写价后走势采样点（market_prices）
  // 已陈旧，强制重拉——不重置去重短路则重拉被吞、留下陈旧点（issue #238）。
  usePricesChanged(() => {
    void forceRefresh()
  })

  onMounted(() => {
    void refresh()
  })

  /** 当前模式的采样点序列（统一形态，供图表与空态消费） */
  const trendPoints = computed(() => {
    if (mode.value === 'portfolio') {
      return (portfolioTrend.value?.points ?? []).map((p) => ({
        date: p.date,
        value: p.market_value_cents,
      }))
    }
    return (instrumentTrend.value?.points ?? []).map((p) => ({
      date: p.date,
      value: p.price_cents,
    }))
  })

  const chartSeries = computed(() => toTrendChartSeries(trendPoints.value))
  const isEmpty = computed(() => isTrendEmpty(trendPoints.value))

  /**
   * 曲线金额币种：组合走势为后端折算的本位币；单标的走势取采样点的报价币种
   * （同一标的价格序列币种恒定，任取一点）。
   */
  const currencyCode = computed(() => {
    if (mode.value === 'portfolio') return portfolioTrend.value?.currency_code ?? null
    return instrumentTrend.value?.points[0]?.currency_code ?? null
  })

  return {
    preset,
    mode,
    instrument,
    instruments,
    loading,
    chartSeries,
    isEmpty,
    currencyCode,
    refresh,
    showInstrument,
    showPortfolio,
  }
}
