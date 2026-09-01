import { describe, it, expect } from 'vitest'
import {
  DATED_TIME_PERIOD_PRESETS,
  TIME_PERIOD_PRESETS,
  matchPreset,
  presetRange,
  type DatedTimePeriodPreset,
} from '@/utils/time-period'

/** 本地日历构造辅助：测试与实现同用本地时区口径，无 UTC 偏移歧义。 */
const d = (y: number, m: number, day: number) => new Date(y, m - 1, day)

describe('time-period 预设闭集', () => {
  it('闭集五项且顺序即芯片渲染顺序：全部 | 当月 | 当季 | 当年 | 去年', () => {
    expect(TIME_PERIOD_PRESETS).toEqual(['all', 'month', 'quarter', 'year', 'lastYear'])
  })

  it('带日期区间的预设子集与全闭集一致（仅缺「全部」）', () => {
    expect(DATED_TIME_PERIOD_PRESETS).toEqual(['month', 'quarter', 'year', 'lastYear'])
  })
})

describe('presetRange：预设 → 含边界日期区间（本地自然周期）', () => {
  it('当月：月中取整月边界', () => {
    expect(presetRange('month', d(2026, 1, 15))).toEqual({ from: '2026-01-01', to: '2026-01-31' })
  })

  it('当月：闰年 2 月含 2 月 29 日', () => {
    expect(presetRange('month', d(2024, 2, 10))).toEqual({ from: '2024-02-01', to: '2024-02-29' })
    expect(presetRange('month', d(2024, 2, 29))).toEqual({ from: '2024-02-01', to: '2024-02-29' })
  })

  it('当月：平年 2 月止于 28 日', () => {
    expect(presetRange('month', d(2023, 2, 28))).toEqual({ from: '2023-02-01', to: '2023-02-28' })
  })

  it('当月：30 天小月与 12 月末', () => {
    expect(presetRange('month', d(2026, 4, 15))).toEqual({ from: '2026-04-01', to: '2026-04-30' })
    expect(presetRange('month', d(2024, 12, 31))).toEqual({ from: '2024-12-01', to: '2024-12-31' })
  })

  it('当季：季初月份 1/4/7/10 分别落到四个自然季度（1–3、4–6、7–9、10–12）', () => {
    expect(presetRange('quarter', d(2026, 1, 1))).toEqual({ from: '2026-01-01', to: '2026-03-31' })
    expect(presetRange('quarter', d(2026, 4, 15))).toEqual({ from: '2026-04-01', to: '2026-06-30' })
    expect(presetRange('quarter', d(2026, 7, 31))).toEqual({ from: '2026-07-01', to: '2026-09-30' })
    expect(presetRange('quarter', d(2026, 10, 1))).toEqual({ from: '2026-10-01', to: '2026-12-31' })
  })

  it('当季：闰年 2 月 29 日仍属一季度，季度边界不受闰月影响', () => {
    expect(presetRange('quarter', d(2024, 2, 29))).toEqual({ from: '2024-01-01', to: '2024-03-31' })
  })

  it('当年：年中、1 月 1 日与 12 月 31 日均取完整自然年', () => {
    expect(presetRange('year', d(2026, 6, 15))).toEqual({ from: '2026-01-01', to: '2026-12-31' })
    expect(presetRange('year', d(2025, 1, 1))).toEqual({ from: '2025-01-01', to: '2025-12-31' })
    expect(presetRange('year', d(2024, 12, 31))).toEqual({ from: '2024-01-01', to: '2024-12-31' })
  })

  it('去年：当前年减一的完整自然年（年初跨年、年末同区间）', () => {
    expect(presetRange('lastYear', d(2026, 1, 1))).toEqual({ from: '2025-01-01', to: '2025-12-31' })
    expect(presetRange('lastYear', d(2026, 12, 31))).toEqual({ from: '2025-01-01', to: '2025-12-31' })
  })

  it('时间戳与 Date 双输入同口径', () => {
    const date = d(2026, 3, 18)
    expect(presetRange('month', date.getTime())).toEqual(presetRange('month', date))
  })
})

describe('matchPreset：当前区间 → 命中预设（高亮派生）', () => {
  const DAILY_PRESETS: readonly DatedTimePeriodPreset[] = DATED_TIME_PERIOD_PRESETS

  it('无日期过滤（双端皆空）= 默认态「全部」点亮', () => {
    expect(matchPreset(null, null, d(2026, 2, 10))).toBe('all')
  })

  it('当前区间恰为某预设定义时命中该预设（字符串区间与换算互逆）', () => {
    const today = d(2026, 2, 10)
    expect(matchPreset('2026-02-01', '2026-02-28', today)).toBe('month')
    expect(matchPreset('2026-01-01', '2026-03-31', today)).toBe('quarter')
    expect(matchPreset('2026-01-01', '2026-12-31', today)).toBe('year')
    expect(matchPreset('2025-01-01', '2025-12-31', today)).toBe('lastYear')
    // 与 presetRange 换算互逆：四个日期预设各自喂回均命中自身
    for (const p of DAILY_PRESETS) {
      const r = presetRange(p, today)
      expect(matchPreset(r.from, r.to, today)).toBe(p)
    }
  })

  it('单端过滤（只起或只止）不命中任何预设', () => {
    expect(matchPreset('2026-02-01', null, d(2026, 2, 10))).toBeNull()
    expect(matchPreset(null, '2026-02-28', d(2026, 2, 10))).toBeNull()
  })

  it('非自然周期边界的任意区间不命中（搜索页任意区间退路的区分度）', () => {
    expect(matchPreset('2026-01-05', '2026-01-20', d(2026, 2, 10))).toBeNull()
  })

  it('跨月/季/年后旧区间不再命中当期预设（高亮熄灭，列表快照不漂移）', () => {
    // 1 月的「当月」区间，进入 2 月后不再点亮
    expect(matchPreset('2026-01-01', '2026-01-31', d(2026, 2, 1))).toBeNull()
    // 历史季度/年份同理
    expect(matchPreset('2025-10-01', '2025-12-31', d(2026, 2, 1))).toBeNull()
    expect(matchPreset('2025-01-01', '2025-12-31', d(2027, 3, 15))).toBeNull()
  })

  it('五个预设的区间两两互异（单选语义：任一区间至多点亮一枚芯片）', () => {
    const today = d(2026, 2, 10)
    const ranges = DAILY_PRESETS.map((p) => presetRange(p, today))
    const keys = new Set(ranges.map((r) => `${r.from}..${r.to}`))
    expect(keys.size).toBe(DAILY_PRESETS.length)
  })
})
