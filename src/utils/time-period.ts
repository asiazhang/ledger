/**
 * 时间范围快捷选择——时间周期纯函数模块（issue #381/#382）。
 *
 * 承载交易页时间维度行的预设闭集、预设 ⇄ 含边界日期区间换算与高亮派生匹配。
 * 周期边界按本地时区日历取自然月 / 自然季度（1–3、4–6、7–9、10–12）/ 自然年，
 * 输出 YYYY-MM-DD 含边界区间（与后端列表查询的 date 字典序比较一致，后端零改动）。
 *
 * 快照语义（#381 设计）：预设只负责换算出精确的自然周期边界，写入过滤模块后即为
 * 快照——跨月/季/年后区间不漂移，仅高亮（matchPreset）随「今天」推移自然熄灭。
 * 本模块不持任何状态，`today` 一律由调用方注入。
 */
import { formatLocalDateISO } from '@/utils/date'

/** 带日期区间的预设闭集（「全部」无区间，单独作默认态哨兵）。 */
export type DatedTimePeriodPreset = 'month' | 'quarter' | 'year' | 'lastYear'

/** 时间维度预设闭集（芯片渲染顺序即数组顺序）：
 * 全部（无日期过滤 = 默认态）| 当月 | 当季 | 当年 | 去年。 */
export type TimePeriodPreset = 'all' | DatedTimePeriodPreset

/** 带日期区间的预设全序（matchPreset 的匹配域）。 */
export const DATED_TIME_PERIOD_PRESETS: readonly DatedTimePeriodPreset[] = [
  'month',
  'quarter',
  'year',
  'lastYear',
]

/** 预设全序（视图按此渲染芯片）。 */
export const TIME_PERIOD_PRESETS: readonly TimePeriodPreset[] = [
  'all',
  ...DATED_TIME_PERIOD_PRESETS,
]

/** 含边界日期区间（YYYY-MM-DD，双端包含）。 */
export interface DateRange {
  from: string
  to: string
}

/** YYYY-MM-DD 格式化复用本地日历日语义单点。 */
const iso = formatLocalDateISO

/** 本地「第 m0 月（0 起）」的月末日后：经「次月 0 日」滚动得出（自动处理闰年）。 */
function lastDayOf(y: number, m0: number): number {
  return new Date(y, m0 + 1, 0).getDate()
}

/** 本地自然月区间：m0 为 0 起月份。 */
function monthRange(y: number, m0: number): DateRange {
  return { from: iso(y, m0, 1), to: iso(y, m0, lastDayOf(y, m0)) }
}

/** 本地自然季度区间：q 为 0 起季度（1–3、4–6、7–9、10–12）。 */
function quarterRange(y: number, q: number): DateRange {
  const startMonth = q * 3
  return { from: iso(y, startMonth, 1), to: iso(y, startMonth + 2, lastDayOf(y, startMonth + 2)) }
}

/** 本地自然年区间。 */
function yearRange(y: number): DateRange {
  return { from: iso(y, 0, 1), to: iso(y, 11, 31) }
}

/** 预设 → 相对 today 的含边界日期区间（本地自然周期；「去年」= 当前年减一的完整自然年）。
 * 时间戳与 Date 双输入。 */
export function presetRange(preset: DatedTimePeriodPreset, today: number | Date): DateRange {
  const d = typeof today === 'number' ? new Date(today) : today
  const y = d.getFullYear()
  switch (preset) {
    case 'month':
      return monthRange(y, d.getMonth())
    case 'quarter':
      return quarterRange(y, Math.floor(d.getMonth() / 3))
    case 'year':
      return yearRange(y)
    case 'lastYear':
      return yearRange(y - 1)
  }
}

/** 高亮派生：当前日期区间恰等于某预设定义（相对 today 的自然周期）时返回该预设；
 * 双端皆空 = 默认态「全部」；单端过滤、任意区间与历史周期区间返回 null（无芯片点亮，
 * 列表快照不漂移）。 */
export function matchPreset(
  from: string | null,
  to: string | null,
  today: number | Date,
): TimePeriodPreset | null {
  if (from === null && to === null) return 'all'
  if (from === null || to === null) return null
  const d = typeof today === 'number' ? new Date(today) : today
  const hit = DATED_TIME_PERIOD_PRESETS.find((p) => {
    const r = presetRange(p, d)
    return r.from === from && r.to === to
  })
  return hit ?? null
}
