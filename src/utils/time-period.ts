/**
 * 时间范围快捷选择——时间周期纯函数模块（issue #381/#382/#383）。
 *
 * 承载交易页时间维度行的预设闭集、预设 ⇄ 含边界日期区间换算、高亮派生匹配，
 * 以及期间步进器（#383）的区间 ⇄（单位，期间）双向换算、期间 ±1 步进与
 * 期间标签本地化格式化。周期边界按本地时区日历取自然月 / 自然季度
 * （1–3、4–6、7–9、10–12）/ 自然年，输出 YYYY-MM-DD 含边界区间
 * （与后端列表查询的 date 字典序比较一致，后端零改动）。
 *
 * 快照语义（#381 设计）：预设与步进只负责换算出精确的自然周期边界，写入过滤
 * 模块后即为快照——跨月/季/年后区间不漂移，仅高亮（matchPreset）随「今天」
 * 推移自然熄灭。期间游标不落状态：步进前从当前区间唯一反推（rangeToPeriod），
 * 步进后写回快照；自然月/季/年跨度互不相同保证反推唯一。
 *
 * 本模块不持任何状态，`today` 一律由调用方注入。
 */
import { formatLocalDateISO } from '@/utils/date'
import { t } from '@/i18n'

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

// ---------------------------------------------------------------------------
// 期间步进器：区间 ⇄（单位，期间）双向换算、步进、标签格式化（issue #383）
// ---------------------------------------------------------------------------

/** 期间单位闭集（与带日期区间的预设单位一一对应）。 */
export type PeriodUnit = 'month' | 'quarter' | 'year'

/** 自然周期（期间步进的游标中间态，不落过滤模块状态）：
 * month → index 为 0 起月份；quarter → index 为 0 起季度；year → index 恒 0。 */
export interface NaturalPeriod {
  unit: PeriodUnit
  year: number
  index: number
}

/** YYYY-MM-DD 解析：非法格式、越界月份与不存在日期（如 2 月 30 日）返回 null。 */
function parseISODate(s: string): { y: number; m0: number; day: number } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s)
  if (!m) return null
  const y = Number(m[1])
  const m0 = Number(m[2]) - 1
  const day = Number(m[3])
  if (m0 < 0 || m0 > 11) return null
  if (day < 1 || day > lastDayOf(y, m0)) return null
  return { y, m0, day }
}

/** 含边界日期区间 → 唯一（单位，期间）：区间恰为某自然月/季/年时返回对应期间。
 * 双端任一为空（「全部」= 默认态）、单端过滤、任意区间与非法日期一律 null
 * （无可步进游标）。自然月/季/年跨度互不相同，反推唯一（单测全枚举覆盖）。 */
export function rangeToPeriod(from: string | null, to: string | null): NaturalPeriod | null {
  if (from === null || to === null) return null
  const f = parseISODate(from)
  const end = parseISODate(to)
  if (!f || !end) return null
  // 年：1 月 1 日 → 同年 12 月 31 日
  if (f.m0 === 0 && f.day === 1 && end.y === f.y && end.m0 === 11 && end.day === 31) {
    return { unit: 'year', year: f.y, index: 0 }
  }
  // 季：同年同季度，且起于季度首月 1 日、止于季度末月最后一日（跨季度区间不命中）
  const fq = Math.floor(f.m0 / 3)
  if (
    end.y === f.y &&
    f.m0 === fq * 3 &&
    end.m0 === fq * 3 + 2 &&
    f.day === 1 &&
    end.day === lastDayOf(end.y, end.m0)
  ) {
    return { unit: 'quarter', year: f.y, index: fq }
  }
  // 月：同年同月，起于 1 日、止于月末（自动兼容闰年 2 月）
  if (f.y === end.y && f.m0 === end.m0 && f.day === 1 && end.day === lastDayOf(end.y, end.m0)) {
    return { unit: 'month', year: f.y, index: f.m0 }
  }
  return null
}

/** 期间 → 含边界日期区间（与 presetRange 共用同一自然周期换算单点，写回快照用）。 */
export function periodRange(p: NaturalPeriod): DateRange {
  switch (p.unit) {
    case 'month':
      return monthRange(p.year, p.index)
    case 'quarter':
      return quarterRange(p.year, p.index)
    case 'year':
      return yearRange(p.year)
  }
}

/** 期间 ±delta 步进（视图只用 ±1）：月/季在年界自然回绕（12 月 → 次年 1 月、
 * 四季度 → 次年一季度），年直接 ±1；不钳制未来期间（空列表是诚实行为）。 */
export function stepPeriod(p: NaturalPeriod, delta: number): NaturalPeriod {
  if (p.unit === 'year') {
    return { unit: 'year', year: p.year + delta, index: 0 }
  }
  const perYear = p.unit === 'month' ? 12 : 4
  const total = p.year * perYear + p.index + delta
  const year = Math.floor(total / perYear)
  return { unit: p.unit, year, index: total - year * perYear }
}

/** 期间标签本地化格式化：zh-CN「2026年2月」「2026年一季度」「2025年」；
 * en-US「Feb 2026」「Q1 2026」「2025」。模板与月/季名称表随交易域文案文件
 * （ADR-0049，无硬编码文案），经 t() 按当前界面语言现取（响应式上下文中调用
 * 随语言切换即时重渲染）。 */
export function formatPeriodLabel(p: NaturalPeriod): string {
  const year = String(p.year)
  if (p.unit === 'year') {
    return t('transactions.filter.periodLabel.year', { year })
  }
  const namesKey = p.unit === 'month' ? 'monthNames' : 'quarterNames'
  const name = t(`transactions.filter.periodLabel.${namesKey}.${p.index + 1}`)
  const key = p.unit === 'month' ? 'month' : 'quarter'
  return t(`transactions.filter.periodLabel.${key}`, { year, name })
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
